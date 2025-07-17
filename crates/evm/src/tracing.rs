//! Helpers for tracing.

use crate::{Evm, IntoTxEnv};
use core::{fmt::Debug, iter::Peekable};
use revm::{
    context::result::{ExecutionResult, ResultAndState},
    state::EvmState,
    DatabaseCommit,
};

/// Trait for inspector initialization.
pub trait InspectorInitializer<T> {
    /// Create a new inspector instance.
    fn initialize(&mut self) -> T;
}

/// A simple initializer that clones the inspector.
#[derive(Debug, Clone)]
pub struct CloneInitializer<T: Clone>(T);

impl<T: Clone> CloneInitializer<T> {
    /// Create a new clone initializer.
    pub fn new(value: T) -> Self {
        Self(value)
    }
}

impl<T: Clone> InspectorInitializer<T> for CloneInitializer<T> {
    fn initialize(&mut self) -> T {
        self.0.clone()
    }
}

/// An initializer that creates inspectors using a closure.
#[derive(Debug)]
pub struct FnInitializer<T, F: FnMut() -> T>(F);

impl<T, F: FnMut() -> T> FnInitializer<T, F> {
    /// Create a new function initializer.
    pub fn new(f: F) -> Self {
        Self(f)
    }
}

impl<T, F: FnMut() -> T> InspectorInitializer<T> for FnInitializer<T, F> {
    fn initialize(&mut self) -> T {
        (self.0)()
    }
}

/// A helper type for tracing transactions.
#[derive(Debug, Clone)]
pub struct TxTracer<
    E: Evm,
    Init: InspectorInitializer<E::Inspector> = CloneInitializer<<E as Evm>::Inspector>,
> {
    evm: E,
    fused_inspector: E::Inspector,
    initializer: Init,
}

/// Container type for context exposed in [`TxTracer`].
#[derive(Debug)]
pub struct TracingCtx<
    'a,
    T,
    E: Evm,
    Init: InspectorInitializer<E::Inspector> = CloneInitializer<<E as Evm>::Inspector>,
> {
    /// The transaction that was just executed.
    pub tx: T,
    /// Result of transaction execution.
    pub result: ExecutionResult<E::HaltReason>,
    /// State changes after transaction.
    pub state: &'a EvmState,
    /// Inspector state after transaction.
    pub inspector: &'a mut E::Inspector,
    /// Database used when executing the transaction, _before_ committing the state changes.
    pub db: &'a mut E::DB,
    /// Fused inspector.
    pub fused_inspector: &'a E::Inspector,
    /// Whether the inspector was fused.
    was_fused: &'a mut bool,
    /// Reference to the initializer.
    initializer: &'a mut Init,
}

impl<'a, T, E: Evm, Init: InspectorInitializer<E::Inspector>> TracingCtx<'a, T, E, Init> {
    /// Takes the current inspector and replaces it with a fresh one.
    ///
    /// This is useful when you want to keep the current inspector state but continue
    /// tracing with a new one.
    pub fn take_inspector(&mut self) -> E::Inspector
    where
        E::Inspector: Clone,
    {
        *self.was_fused = true;
        core::mem::replace(self.inspector, self.initializer.initialize())
    }
}

impl<E: Evm<DB: DatabaseCommit>, Init: InspectorInitializer<E::Inspector>> TxTracer<E, Init> {
    /// Creates a new [`TxTracer`] instance with a custom inspector initializer.
    pub fn new_with_initializer(evm: E, mut initializer: Init) -> Self {
        let fused_inspector = initializer.initialize();
        Self { fused_inspector, evm, initializer }
    }

    fn fuse_inspector(&mut self) -> E::Inspector {
        let fresh = self.initializer.initialize();
        core::mem::replace(self.evm.inspector_mut(), fresh)
    }

    /// Executes a transaction, and returns its outcome along with the inspector state.
    pub fn trace(
        &mut self,
        tx: impl IntoTxEnv<E::Tx>,
    ) -> Result<TraceOutput<E::HaltReason, E::Inspector>, E::Error>
    where
        E::Inspector: Clone,
    {
        let result = self.evm.transact_commit(tx);
        let inspector = self.fuse_inspector();
        Ok(TraceOutput { result: result?, inspector })
    }

    /// Executes multiple transactions, applies the closure to each transaction result, and returns
    /// the outcomes.
    #[expect(clippy::type_complexity)]
    pub fn trace_many<Txs, T, F, O>(
        &mut self,
        txs: Txs,
        mut f: F,
    ) -> TracerIter<
        '_,
        E,
        Init,
        Txs::IntoIter,
        impl FnMut(TracingCtx<'_, T, E, Init>) -> Result<O, E::Error>,
    >
    where
        T: IntoTxEnv<E::Tx> + Clone,
        Txs: IntoIterator<Item = T>,
        F: FnMut(TracingCtx<'_, Txs::Item, E, Init>) -> O,
    {
        self.try_trace_many(txs, move |ctx| Ok(f(ctx)))
    }

    /// Same as [`TxTracer::trace_many`], but operates on closures returning [`Result`]s.
    pub fn try_trace_many<Txs, T, F, O, Err>(
        &mut self,
        txs: Txs,
        hook: F,
    ) -> TracerIter<'_, E, Init, Txs::IntoIter, F>
    where
        T: IntoTxEnv<E::Tx> + Clone,
        Txs: IntoIterator<Item = T>,
        F: FnMut(TracingCtx<'_, T, E, Init>) -> Result<O, Err>,
        Err: From<E::Error>,
    {
        TracerIter {
            inner: self,
            txs: txs.into_iter().peekable(),
            hook,
            skip_last_commit: true,
            fuse: true,
        }
    }
}

impl<E: Evm<DB: DatabaseCommit>> TxTracer<E, CloneInitializer<E::Inspector>>
where
    E::Inspector: Clone,
{
    /// Creates a new [`TxTracer`] instance with a cloneable inspector.
    pub fn new(mut evm: E) -> Self {
        let inspector = evm.inspector_mut().clone();
        Self::new_with_initializer(evm, CloneInitializer::new(inspector))
    }
}

/// Output of tracing a transaction.
#[derive(Debug, Clone)]
pub struct TraceOutput<H, I> {
    /// Inner EVM output.
    pub result: ExecutionResult<H>,
    /// Inspector state at the end of the execution.
    pub inspector: I,
}

/// Iterator used by tracer.
#[derive(derive_more::Debug)]
#[debug(bound(E::Inspector: Debug))]
pub struct TracerIter<'a, E: Evm, Init: InspectorInitializer<E::Inspector>, Txs: Iterator, F> {
    inner: &'a mut TxTracer<E, Init>,
    txs: Peekable<Txs>,
    hook: F,
    skip_last_commit: bool,
    fuse: bool,
}

impl<E: Evm, Init: InspectorInitializer<E::Inspector>, Txs: Iterator, F>
    TracerIter<'_, E, Init, Txs, F>
{
    /// Flips the `skip_last_commit` flag thus making sure all transaction are committed.
    ///
    /// We are skipping last commit by default as it's expected that when tracing users are mostly
    /// interested in tracer output rather than in a state after it.
    pub fn commit_last_tx(mut self) -> Self {
        self.skip_last_commit = false;
        self
    }

    /// Disables inspector fusing on every transaction and expects user to fuse it manually.
    pub fn no_fuse(mut self) -> Self {
        self.fuse = false;
        self
    }
}

impl<E, Init, T, Txs, F, O, Err> Iterator for TracerIter<'_, E, Init, Txs, F>
where
    E: Evm<DB: DatabaseCommit>,
    Init: InspectorInitializer<E::Inspector>,
    T: IntoTxEnv<E::Tx> + Clone,
    Txs: Iterator<Item = T>,
    Err: From<E::Error>,
    F: FnMut(TracingCtx<'_, T, E, Init>) -> Result<O, Err>,
{
    type Item = Result<O, Err>;

    fn next(&mut self) -> Option<Self::Item> {
        let tx = self.txs.next()?;
        let result = self.inner.evm.transact(tx.clone());

        let TxTracer { evm, fused_inspector, initializer } = self.inner;
        let (db, inspector, _) = evm.components_mut();

        let Ok(ResultAndState { result, state }) = result else {
            return None;
        };
        let mut was_fused = false;
        let output = (self.hook)(TracingCtx {
            tx,
            result,
            state: &state,
            inspector,
            db,
            fused_inspector: &*fused_inspector,
            was_fused: &mut was_fused,
            initializer,
        });

        // Only commit next transaction if `skip_last_commit` is disabled or there is a next
        // transaction.
        if !self.skip_last_commit || self.txs.peek().is_some() {
            db.commit(state);
        }

        if self.fuse && !was_fused {
            self.inner.fuse_inspector();
        }

        Some(output)
    }
}
