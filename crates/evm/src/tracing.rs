//! Helpers for tracing.

use crate::{Evm, IntoTxEnv};
use revm::{
    context::result::{ExecutionResult, ResultAndState},
    DatabaseCommit,
};

/// A helper type for tracing transactions.
#[derive(Debug, Clone)]
pub struct TxTracer<E: Evm> {
    evm: E,
    fused_inspector: E::Inspector,
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
    pub fn trace_many<T, O>(
        &mut self,
        txs: impl IntoIterator<Item = T>,
        mut f: impl FnMut(T, ResultAndState<E::HaltReason>, E::Inspector, &mut E::DB) -> O,
    ) -> Result<Vec<O>, E::Error>
    where
        T: IntoTxEnv<E::Tx> + Clone,
    {
        self.try_trace_many(txs, |tx, result, inspector, db| Ok(f(tx, result, inspector, db)))
    }

    /// Same as [`TxTracer::trace_many`], but operates on closures returning [`Result`]s.
    pub fn try_trace_many<T, O, Err>(
        &mut self,
        txs: impl IntoIterator<Item = T>,
        mut f: impl FnMut(T, ResultAndState<E::HaltReason>, E::Inspector, &mut E::DB) -> Result<O, Err>,
    ) -> Result<Vec<O>, Err>
    where
        T: IntoTxEnv<E::Tx> + Clone,
        Err: From<E::Error>,
    {
        let mut outputs = Vec::new();

        for tx in txs {
            let result = self.evm.transact(tx.clone());
            let inspector = self.fuse_inspector();
            outputs.push(f(tx, result?, inspector, self.evm.db_mut())?);
        }

        Ok(outputs)
    }
}
