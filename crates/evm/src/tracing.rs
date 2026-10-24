//! Helpers for tracing.

use crate::{Evm, IntoTxEnv};
use alloc::vec::Vec;
use core::{fmt::Debug, iter::Peekable};
use revm::{
    context::result::{ExecutionResult, ResultAndState},
    state::EvmState,
    DatabaseCommit,
};

/// Output of [`BlockTracer::trace_transaction`].
pub type TraceTransactionOutput<E> =
    Option<(usize, TraceOutput<<E as Evm>::HaltReason, <E as Evm>::Inspector>)>;

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

    /// Returns a mutable reference to the inner EVM.
    pub const fn evm_mut(&mut self) -> &mut E {
        &mut self.evm
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

/// Traces a subset of a block's transactions, replaying earlier ones without tracing.
///
/// This type wraps [`TxTracer`] and adds skip-then-trace semantics: transactions before the
/// target range are committed to state without invoking the inspector or any user hook,
/// and only the target range is traced.
#[derive(derive_more::Debug)]
#[debug(bound(E::Inspector: core::fmt::Debug))]
pub struct BlockTracer<E: Evm> {
    tracer: TxTracer<E>,
}

impl<E: Evm<Inspector: Clone> + Clone> Clone for BlockTracer<E> {
    fn clone(&self) -> Self {
        Self { tracer: self.tracer.clone() }
    }
}

impl<E: Evm<Inspector: Clone, DB: DatabaseCommit>> BlockTracer<E> {
    /// Creates a new [`BlockTracer`].
    ///
    /// The caller is responsible for applying pre-execution changes (beacon root, DAO fork,
    /// etc.) to the database before calling this.
    pub fn new(evm: E) -> Self {
        Self { tracer: TxTracer::new(evm) }
    }

    /// Replays `txs[0..skip)` without tracing (commits state), then traces
    /// `txs[skip..skip+count)` — or all remaining if `count` is `None` — calling `f` for
    /// each traced transaction.
    ///
    /// Returns a [`Vec`] of the outputs produced by `f`.
    pub fn try_trace_block<Txs, T, F, O, Err>(
        &mut self,
        txs: Txs,
        skip: usize,
        count: Option<usize>,
        f: F,
    ) -> Result<Vec<O>, Err>
    where
        T: IntoTxEnv<E::Tx> + Clone,
        Txs: IntoIterator<Item = T>,
        F: FnMut(TracingCtx<'_, T, E>) -> Result<O, Err>,
        Err: From<E::Error>,
    {
        let mut txs = txs.into_iter();

        // Commit the first `skip` transactions without tracing.
        for tx in txs.by_ref().take(skip) {
            self.tracer.evm_mut().transact_commit(tx)?;
        }

        // Trace the remaining transactions, optionally limited by `count`.
        let traced = txs.take(count.unwrap_or(usize::MAX));
        self.tracer.try_trace_many(traced, f).collect()
    }

    /// Replays all transactions before the one matched by `target_tx` without tracing, then
    /// traces that transaction alone.
    ///
    /// Returns `Some((index, output))` where `index` is the position of the matched
    /// transaction in the iterator, or `None` if `target_tx` never matched.
    pub fn trace_transaction<Txs, T>(
        &mut self,
        txs: Txs,
        target_tx: impl Fn(&T) -> bool,
    ) -> Result<TraceTransactionOutput<E>, E::Error>
    where
        T: IntoTxEnv<E::Tx> + Clone,
        Txs: IntoIterator<Item = T>,
    {
        for (index, tx) in txs.into_iter().enumerate() {
            if target_tx(&tx) {
                let output = self.tracer.trace(tx)?;
                return Ok(Some((index, output)));
            }
            self.tracer.evm_mut().transact_commit(tx)?;
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        eth::{EthEvm, EthEvmFactory},
        precompiles::PrecompilesMap,
        EvmEnv, EvmFactory,
    };
    use alloy_primitives::{Address, TxKind, U256};
    use core::convert::Infallible;
    use revm::{
        context::{BlockEnv, CfgEnv, TxEnv},
        context_interface::result::EVMError,
        database::InMemoryDB,
        inspector::NoOpInspector,
        primitives::hardfork::SpecId,
        state::AccountInfo,
        Database,
    };

    type TestEvm = EthEvm<InMemoryDB, NoOpInspector, PrecompilesMap>;

    fn make_env() -> EvmEnv {
        let mut cfg = CfgEnv::<SpecId>::default();
        cfg.disable_nonce_check = true;
        cfg.tx_chain_id_check = false;
        EvmEnv::new(cfg, BlockEnv::default())
    }

    /// Skipped transactions must commit their state so that traced transactions see
    /// the accumulated result. Here 2 txs are skipped, each transferring 1 wei to
    /// `recipient`; the single traced tx observes a recipient balance of 2.
    #[test]
    fn test_skip_commits_state() {
        let sender = Address::from([1u8; 20]);
        let recipient = Address::from([2u8; 20]);

        let mut db = InMemoryDB::default();
        db.insert_account_info(sender, AccountInfo { balance: U256::MAX, ..Default::default() });

        let mut tracer: BlockTracer<TestEvm> =
            BlockTracer::new(EthEvmFactory.create_evm(db, make_env()));

        let txs: Vec<TxEnv> = (0..3)
            .map(|nonce| TxEnv {
                nonce,
                caller: sender,
                gas_limit: 21_000,
                kind: TxKind::Call(recipient),
                value: U256::from(1),
                ..Default::default()
            })
            .collect();

        // Skip 2, trace 1. The hook should see recipient balance = 2 from the skipped txs.
        let results = tracer
            .try_trace_block(txs, 2, None, |ctx| {
                let balance = ctx.db.basic(recipient).unwrap().map_or(U256::ZERO, |a| a.balance);
                Ok::<_, EVMError<Infallible>>(balance)
            })
            .unwrap();

        assert_eq!(results, [U256::from(2)]);
    }

    /// trace_transaction commits all prior txs and returns the correct index.
    #[test]
    fn test_trace_transaction_index() {
        let sender = Address::from([1u8; 20]);

        let mut db = InMemoryDB::default();
        db.insert_account_info(sender, AccountInfo { balance: U256::MAX, ..Default::default() });

        let mut tracer: BlockTracer<TestEvm> =
            BlockTracer::new(EthEvmFactory.create_evm(db, make_env()));

        let txs: Vec<TxEnv> = (0u64..5)
            .map(|nonce| TxEnv {
                nonce,
                caller: sender,
                gas_limit: 21_000,
                kind: TxKind::Call(Address::ZERO),
                ..Default::default()
            })
            .collect();

        let (idx, output) = tracer.trace_transaction(txs, |t| t.nonce == 3).unwrap().unwrap();

        assert_eq!(idx, 3);
        assert!(output.result.is_success());
    }
}
