//! Helpers for tracing.

use crate::{Evm, IntoTxEnv};
use revm::context::result::ResultAndState;

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
    pub result: ResultAndState<H>,
    /// Inspector state at the end of the execution.
    pub inspector: I,
}

impl<E: Evm<Inspector: Clone>> TxTracer<E> {
    /// Creates a new [`TxTracer`] instance.
    pub fn new(mut evm: E) -> Self {
        Self { fused_inspector: evm.inspector_mut().clone(), evm }
    }

    /// Executes a transaction, and returns its outcome along with the inspector state.
    pub fn trace(
        &mut self,
        tx: impl IntoTxEnv<E::Tx>,
    ) -> Result<TraceOutput<E::HaltReason, E::Inspector>, E::Error> {
        let result = self.evm.transact(tx);
        let inspector = core::mem::replace(self.evm.inspector_mut(), self.fused_inspector.clone());
        Ok(TraceOutput { result: result?, inspector })
    }
}
