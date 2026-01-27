//! Helpers for tracing.

use crate::{
    block::{BlockExecutionError, BlockExecutor, BlockExecutorFactory, BlockExecutorFor},
    Database, Evm, EvmFactory, IntoTxEnv,
};
use core::{fmt::Debug, iter::Peekable};
use revm::{
    context::result::{ExecutionResult, ResultAndState},
    database::State,
    state::EvmState,
    DatabaseCommit, Inspector,
};

/// Error that can occur during block tracing iteration.
#[derive(Debug, thiserror::Error)]
pub enum BlockTracingError<EvmErr, HookErr> {
    /// Error during pre-execution changes (system calls, etc.)
    #[error("pre-execution error: {0}")]
    PreExecution(BlockExecutionError),
    /// EVM error during transaction execution.
    #[error("evm error: {0}")]
    Evm(EvmErr),
    /// Error from the tracing hook.
    #[error(transparent)]
    Hook(HookErr),
}

impl<EvmErr, HookErr> BlockTracingError<EvmErr, HookErr> {
    /// Returns the pre-execution error if this is a [`BlockTracingError::PreExecution`] variant.
    pub const fn as_pre_execution(&self) -> Option<&BlockExecutionError> {
        match self {
            Self::PreExecution(err) => Some(err),
            _ => None,
        }
    }

    /// Returns the EVM error if this is a [`BlockTracingError::Evm`] variant.
    pub const fn as_evm(&self) -> Option<&EvmErr> {
        match self {
            Self::Evm(err) => Some(err),
            _ => None,
        }
    }

    /// Returns the hook error if this is a [`BlockTracingError::Hook`] variant.
    pub const fn as_hook(&self) -> Option<&HookErr> {
        match self {
            Self::Hook(err) => Some(err),
            _ => None,
        }
    }
}

/// A helper type for tracing transactions.
#[derive(Debug, Clone)]
pub struct TxTracer<E: Evm> {
    evm: E,
    fused_inspector: E::Inspector,
}

/// Container type for context exposed in [`TxTracer`].
#[derive(Debug)]
pub struct TracingCtx<'a, T, E: Evm> {
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
    fused_inspector: &'a E::Inspector,
    /// Whether the inspector was fused.
    was_fused: &'a mut bool,
}

impl<'a, T, E: Evm<Inspector: Clone>> TracingCtx<'a, T, E> {
    /// Fuses the inspector and returns the current inspector state.
    pub fn take_inspector(&mut self) -> E::Inspector {
        *self.was_fused = true;
        core::mem::replace(self.inspector, self.fused_inspector.clone())
    }
}

impl<E: Evm<Inspector: Clone, DB: DatabaseCommit>> TxTracer<E> {
    /// Creates a new [`TxTracer`] instance.
    pub fn new(mut evm: E) -> Self {
        Self { fused_inspector: evm.inspector_mut().clone(), evm }
    }

    fn fuse_inspector(&mut self) -> E::Inspector {
        core::mem::replace(self.evm.inspector_mut(), self.fused_inspector.clone())
    }

    /// Executes a transaction, and returns its outcome along with the inspector state.
    pub fn trace(
        &mut self,
        tx: impl IntoTxEnv<E::Tx>,
    ) -> Result<TraceOutput<E::HaltReason, E::Inspector>, E::Error> {
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
    ) -> TracerIter<'_, E, Txs::IntoIter, impl FnMut(TracingCtx<'_, T, E>) -> Result<O, E::Error>>
    where
        T: IntoTxEnv<E::Tx> + Clone,
        Txs: IntoIterator<Item = T>,
        F: FnMut(TracingCtx<'_, Txs::Item, E>) -> O,
    {
        self.try_trace_many(txs, move |ctx| Ok(f(ctx)))
    }

    /// Same as [`TxTracer::trace_many`], but operates on closures returning [`Result`]s.
    pub fn try_trace_many<Txs, T, F, O, Err>(
        &mut self,
        txs: Txs,
        hook: F,
    ) -> TracerIter<'_, E, Txs::IntoIter, F>
    where
        T: IntoTxEnv<E::Tx> + Clone,
        Txs: IntoIterator<Item = T>,
        F: FnMut(TracingCtx<'_, T, E>) -> Result<O, Err>,
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
pub struct TracerIter<'a, E: Evm, Txs: Iterator, F> {
    inner: &'a mut TxTracer<E>,
    txs: Peekable<Txs>,
    hook: F,
    skip_last_commit: bool,
    fuse: bool,
}

impl<E: Evm, Txs: Iterator, F> TracerIter<'_, E, Txs, F> {
    /// Flips the `skip_last_commit` flag thus making sure all transaction are committed.
    ///
    /// We are skipping last commit by default as it's expected that when tracing users are mostly
    /// interested in tracer output rather than in a state after it.
    pub const fn commit_last_tx(mut self) -> Self {
        self.skip_last_commit = false;
        self
    }

    /// Disables inspector fusing on every transaction and expects user to fuse it manually.
    pub const fn no_fuse(mut self) -> Self {
        self.fuse = false;
        self
    }
}

impl<E, T, Txs, F, O, Err> Iterator for TracerIter<'_, E, Txs, F>
where
    E: Evm<DB: DatabaseCommit, Inspector: Clone>,
    T: IntoTxEnv<E::Tx> + Clone,
    Txs: Iterator<Item = T>,
    Err: From<E::Error>,
    F: FnMut(TracingCtx<'_, T, E>) -> Result<O, Err>,
{
    type Item = Result<O, Err>;

    fn next(&mut self) -> Option<Self::Item> {
        let tx = self.txs.next()?;
        let result = self.inner.evm.transact(tx.clone());

        let TxTracer { evm, fused_inspector } = self.inner;
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

/// A helper type for tracing transactions in the context of block execution.
///
/// This type wraps a [`BlockExecutor`] and provides tracing capabilities similar to [`TxTracer`],
/// but operates within a block execution context. It allows:
/// - Calling [`BlockExecutor::apply_pre_execution_changes`] before tracing transactions
/// - Accessing the block executor's state during tracing
/// - Finishing block execution after tracing with [`BlockExecutor::finish`]
#[derive(derive_more::Debug)]
#[debug(bound(E: Debug, <E::Evm as Evm>::Inspector: Debug))]
pub struct BlockTracer<E: BlockExecutor> {
    executor: E,
    fused_inspector: <E::Evm as Evm>::Inspector,
}

impl<E> BlockTracer<E>
where
    E: BlockExecutor,
    E::Evm: Evm<Inspector: Clone>,
{
    /// Creates a new [`BlockTracer`] instance.
    pub fn new(mut executor: E) -> Self {
        Self { fused_inspector: executor.evm_mut().inspector_mut().clone(), executor }
    }

    fn fuse_inspector(&mut self) -> <E::Evm as Evm>::Inspector {
        core::mem::replace(self.executor.evm_mut().inspector_mut(), self.fused_inspector.clone())
    }

    /// Returns a reference to the underlying [`BlockExecutor`].
    pub const fn executor(&self) -> &E {
        &self.executor
    }

    /// Returns a mutable reference to the underlying [`BlockExecutor`].
    pub const fn executor_mut(&mut self) -> &mut E {
        &mut self.executor
    }

    /// Applies any necessary changes before executing the block's transactions.
    ///
    /// This delegates to [`BlockExecutor::apply_pre_execution_changes`].
    pub fn apply_pre_execution_changes(&mut self) -> Result<(), BlockExecutionError> {
        self.executor.apply_pre_execution_changes()
    }

    /// Finishes block execution and returns the underlying EVM along with execution result.
    ///
    /// This delegates to [`BlockExecutor::finish`].
    pub fn finish(
        self,
    ) -> Result<(E::Evm, crate::block::BlockExecutionResult<E::Receipt>), BlockExecutionError> {
        self.executor.finish()
    }
}

impl<E> BlockTracer<E>
where
    E: BlockExecutor,
    E::Evm: Evm<Inspector: Clone, DB: DatabaseCommit>,
{
    /// Executes a transaction, and returns its outcome along with the inspector state.
    #[expect(clippy::type_complexity)]
    pub fn trace(
        &mut self,
        tx: impl IntoTxEnv<<E::Evm as Evm>::Tx>,
    ) -> Result<
        TraceOutput<<E::Evm as Evm>::HaltReason, <E::Evm as Evm>::Inspector>,
        <E::Evm as Evm>::Error,
    > {
        let result = self.executor.evm_mut().transact_commit(tx);
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
    ) -> BlockTracerIter<
        '_,
        E,
        Txs::IntoIter,
        impl FnMut(TracingCtx<'_, T, E::Evm>) -> Result<O, <E::Evm as Evm>::Error>,
    >
    where
        T: IntoTxEnv<<E::Evm as Evm>::Tx> + Clone,
        Txs: IntoIterator<Item = T>,
        F: FnMut(TracingCtx<'_, Txs::Item, E::Evm>) -> O,
    {
        self.try_trace_many(txs, move |ctx| Ok(f(ctx)))
    }

    /// Same as [`BlockTracer::trace_many`], but operates on closures returning [`Result`]s.
    pub fn try_trace_many<Txs, T, F, O, Err>(
        &mut self,
        txs: Txs,
        hook: F,
    ) -> BlockTracerIter<'_, E, Txs::IntoIter, F>
    where
        T: IntoTxEnv<<E::Evm as Evm>::Tx> + Clone,
        Txs: IntoIterator<Item = T>,
        F: FnMut(TracingCtx<'_, T, E::Evm>) -> Result<O, Err>,
    {
        BlockTracerIter {
            inner: self,
            txs: txs.into_iter().peekable(),
            hook,
            skip_last_commit: true,
            fuse: true,
            apply_pre_execution: true,
            done: false,
        }
    }
}

/// Iterator used by block tracer.
#[derive(derive_more::Debug)]
#[debug(bound(<E::Evm as Evm>::Inspector: Debug))]
pub struct BlockTracerIter<'a, E: BlockExecutor, Txs: Iterator, F> {
    inner: &'a mut BlockTracer<E>,
    txs: Peekable<Txs>,
    hook: F,
    skip_last_commit: bool,
    fuse: bool,
    apply_pre_execution: bool,
    done: bool,
}

impl<E: BlockExecutor, Txs: Iterator, F> BlockTracerIter<'_, E, Txs, F> {
    /// Flips the `skip_last_commit` flag thus making sure all transaction are committed.
    pub const fn commit_last_tx(mut self) -> Self {
        self.skip_last_commit = false;
        self
    }

    /// Disables inspector fusing on every transaction and expects user to fuse it manually.
    pub const fn no_fuse(mut self) -> Self {
        self.fuse = false;
        self
    }

    /// Disables automatic `apply_pre_execution_changes` call on first iteration.
    ///
    /// By default, the iterator will call [`BlockExecutor::apply_pre_execution_changes`] before
    /// processing the first transaction, with the inspector disabled during this call.
    /// Use this method if you want to handle pre-execution changes manually.
    pub const fn skip_pre_execution(mut self) -> Self {
        self.apply_pre_execution = false;
        self
    }
}

impl<E, T, Txs, F, O, HookErr> Iterator for BlockTracerIter<'_, E, Txs, F>
where
    E: BlockExecutor,
    E::Evm: Evm<DB: DatabaseCommit, Inspector: Clone>,
    T: IntoTxEnv<<E::Evm as Evm>::Tx> + Clone,
    Txs: Iterator<Item = T>,
    F: FnMut(TracingCtx<'_, T, E::Evm>) -> Result<O, HookErr>,
{
    type Item = Result<O, BlockTracingError<<E::Evm as Evm>::Error, HookErr>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        if self.apply_pre_execution {
            self.apply_pre_execution = false;
            self.inner.executor.evm_mut().disable_inspector();
            let result = self.inner.executor.apply_pre_execution_changes();
            self.inner.executor.evm_mut().enable_inspector();
            if let Err(err) = result {
                self.done = true;
                return Some(Err(BlockTracingError::PreExecution(err)));
            }
        }

        let tx = self.txs.next()?;
        let result = self.inner.executor.evm_mut().transact(tx.clone());

        let BlockTracer { executor, fused_inspector } = self.inner;
        let (db, inspector, _) = executor.evm_mut().components_mut();

        let ResultAndState { result, state } = match result {
            Ok(res) => res,
            Err(err) => {
                self.done = true;
                return Some(Err(BlockTracingError::Evm(err)));
            }
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
        });

        if !self.skip_last_commit || self.txs.peek().is_some() {
            db.commit(state);
        }

        if self.fuse && !was_fused {
            self.inner.fuse_inspector();
        }

        Some(output.map_err(BlockTracingError::Hook))
    }
}

/// An extension trait for [`BlockExecutorFactory`] providing tracing helpers.
pub trait BlockExecutorFactoryExt: BlockExecutorFactory {
    /// Creates a new [`BlockTracer`] instance with the given state, input, execution context and
    /// inspector.
    fn create_block_tracer<'a, DB, I>(
        &'a self,
        state: &'a mut State<DB>,
        input: crate::EvmEnv<
            <Self::EvmFactory as EvmFactory>::Spec,
            <Self::EvmFactory as EvmFactory>::BlockEnv,
        >,
        ctx: Self::ExecutionCtx<'a>,
        fused_inspector: I,
    ) -> BlockTracer<impl BlockExecutorFor<'a, Self, DB, I> + 'a>
    where
        DB: Database + 'a,
        I: Inspector<<Self::EvmFactory as EvmFactory>::Context<&'a mut State<DB>>> + Clone + 'a,
    {
        let evm = self.evm_factory().create_evm_with_inspector(state, input, fused_inspector);
        BlockTracer::new(self.create_executor(evm, ctx))
    }
}

impl<T: BlockExecutorFactory> BlockExecutorFactoryExt for T {}
