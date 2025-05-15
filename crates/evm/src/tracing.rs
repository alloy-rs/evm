//! Helpers for tracing.

use crate::{
    block::{BlockExecutionError, BlockExecutor, ExecutableTx},
    Evm, IntoTxEnv,
};
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

/// A helper type for tracing entire blocks.
#[derive(derive_more::Debug)]
#[debug(bound(<E::Evm as Evm>::Inspector: core::fmt::Debug))]
pub struct BlockTracer<E: BlockExecutor> {
    executor: E,
    fused_inspector: <E::Evm as Evm>::Inspector,
}

impl<E: BlockExecutor<Evm: Evm<Inspector: Clone>>> BlockTracer<E> {
    /// Creates a new [`BlockTracer`] instance.
    pub fn new(mut executor: E) -> Self {
        Self { fused_inspector: executor.evm_mut().inspector_mut().clone(), executor }
    }

    fn fuse_inspector(&mut self) -> <E::Evm as Evm>::Inspector {
        core::mem::replace(self.executor.evm_mut().inspector_mut(), self.fused_inspector.clone())
    }

    /// Executes a block with the configured inspector and applies the closure to each transaction
    /// result.
    pub fn trace_block<T: ExecutableTx<E>, O>(
        mut self,
        transactions: impl IntoIterator<Item = T>,
        f: impl Fn(T, u64, <E::Evm as Evm>::Inspector) -> O,
    ) -> Result<Vec<O>, BlockExecutionError>
    where
        E::Evm: Evm<Inspector: Clone>,
    {
        // Apply pre-execution changes with the inspector disabled.
        self.executor.evm_mut().disable_inspector();
        self.executor.apply_pre_execution_changes()?;
        self.executor.evm_mut().enable_inspector();

        let mut outputs = Vec::new();

        // Execute all transactions.
        for tx in transactions {
            let gas_used = self.executor.execute_transaction(tx)?;
            let inspector = self.fuse_inspector();
            outputs.push(f(tx, gas_used, inspector));
        }

        // Apply post-execution changes with the inspector disabled.
        self.executor.evm_mut().disable_inspector();
        self.executor.apply_post_execution_changes()?;

        Ok(outputs)
    }
}
