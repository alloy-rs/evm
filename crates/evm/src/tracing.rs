//! Helpers for tracing.

use crate::{Evm, IntoTxEnv};
use revm::{
    context::result::{ExecutionResult, ResultAndState},
    state::EvmState,
    DatabaseCommit,
};

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
    pub inspector: E::Inspector,
    /// Database used when executing the transaction, _before_ committing the state changes.
    pub db: &'a mut E::DB,
}

/// Output of tracing a transaction.
#[derive(Debug, Clone)]
pub struct TraceOutput<H, I> {
    /// Inner EVM output.
    pub result: ExecutionResult<H>,
    /// Inspector state at the end of the execution.
    pub inspector: I,
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
    pub fn trace_many<'a, T, O>(
        &'a mut self,
        txs: impl IntoIterator<Item = T, IntoIter: 'a>,
        mut f: impl FnMut(TracingCtx<'_, T, E>) -> O + 'a,
    ) -> impl Iterator<Item = Result<O, E::Error>> + 'a
    where
        T: IntoTxEnv<E::Tx> + Clone,
    {
        self.try_trace_many(txs, move |ctx| Ok(f(ctx)))
    }

    /// Same as [`TxTracer::trace_many`], but operates on closures returning [`Result`]s.
    pub fn try_trace_many<'a, T, O, Err>(
        &'a mut self,
        txs: impl IntoIterator<Item = T, IntoIter: 'a>,
        mut f: impl FnMut(TracingCtx<'_, T, E>) -> Result<O, Err> + 'a,
    ) -> impl Iterator<Item = Result<O, Err>> + 'a
    where
        T: IntoTxEnv<E::Tx> + Clone,
        Err: From<E::Error>,
    {
        txs.into_iter().map(move |tx| {
            let result = self.evm.transact(tx.clone());
            let inspector = self.fuse_inspector();
            let ResultAndState { result, state } = result?;
            let output =
                f(TracingCtx { tx, result, state: &state, inspector, db: self.evm.db_mut() });
            self.evm.db_mut().commit(state);

            output
        })
    }
}
