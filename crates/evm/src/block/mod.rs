//! Block execution abstraction.

use crate::Evm;
use alloc::{boxed::Box, vec::Vec};
use alloy_consensus::transaction::Recovered;
use alloy_eips::eip7685::Requests;
use revm::context::result::ExecutionResult;

mod error;
pub use error::*;

mod state_hook;
pub use state_hook::*;

/// The result of executing a block.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlockExecutionResult<T> {
    /// All the receipts of the transactions in the block.
    pub receipts: Vec<T>,
    /// All the EIP-7685 requests in the block.
    pub requests: Requests,
    /// The total gas used by the block.
    pub gas_used: u64,
}

/// A type that knows how to execute g a single block.
///
/// The current abstraction assumes that block execution consists of the following steps:
/// 1. Apply pre-execution changes. Those might include system calls, irregular state transitions
///    (DAO fork), etc.
/// 2. Apply block transactions to the state.
/// 3. Apply post-execution changes and finalize the state. This might include other system calls,
///    block rewards, etc.
///
/// The output of [`BlockExecutor::finish`] is a [`BlockExecutionResult`] which contains all
/// relevant information about the block execution.
pub trait BlockExecutor {
    /// Input transaction type.
    type Transaction;
    /// Receipt type this executor produces.
    type Receipt;
    /// EVM used by the executor.
    type Evm: Evm;

    /// Applies any necessary changes before executing the block's transactions.
    fn apply_pre_execution_changes(&mut self) -> Result<(), BlockExecutionError>;

    /// Executes a single transaction and applies execution result to internal state.
    ///
    /// Returns the gas used by the transaction.
    fn execute_transaction(
        &mut self,
        tx: Recovered<&Self::Transaction>,
    ) -> Result<u64, BlockExecutionError> {
        self.execute_transaction_with_result_closure(tx, |_| ())
    }

    /// Executes a single transaction and applies execution result to internal state. Invokes the
    /// given closure with an internal [`ExecutionResult`] produced by the EVM.
    fn execute_transaction_with_result_closure(
        &mut self,
        tx: Recovered<&Self::Transaction>,
        f: impl FnOnce(&ExecutionResult<<Self::Evm as Evm>::HaltReason>),
    ) -> Result<u64, BlockExecutionError>;

    /// Applies any necessary changes after executing the block's transactions, completes execution
    /// and returns the underlying EVM along with execution result.
    fn finish(
        self,
    ) -> Result<(Self::Evm, BlockExecutionResult<Self::Receipt>), BlockExecutionError>;

    /// A helper to invoke [`BlockExecutor::finish`] returning only the [`BlockExecutionResult`].
    fn apply_post_execution_changes(
        self,
    ) -> Result<BlockExecutionResult<Self::Receipt>, BlockExecutionError>
    where
        Self: Sized,
    {
        self.finish().map(|(_, result)| result)
    }

    /// Sets a hook to be called after each state change during execution.
    fn set_state_hook(&mut self, hook: Option<Box<dyn OnStateHook>>);

    /// A builder-style helper to invoke [`BlockExecutor::set_state_hook`].
    #[must_use]
    fn with_state_hook(mut self, hook: Option<Box<dyn OnStateHook>>) -> Self
    where
        Self: Sized,
    {
        self.set_state_hook(hook);
        self
    }

    /// Exposes mutable reference to EVM.
    fn evm_mut(&mut self) -> &mut Self::Evm;
}
