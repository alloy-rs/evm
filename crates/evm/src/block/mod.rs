//! Block execution abstraction.

use crate::{Database, Evm, EvmFactory, FromRecoveredTx, IntoTxEnv, RecoveredTx};
use alloc::{boxed::Box, vec::Vec};
use alloy_eips::eip7685::Requests;
use revm::{
    context::result::ExecutionResult, database::State, inspector::NoOpInspector, Inspector,
};

mod error;
pub use error::*;

mod state_hook;
pub use state_hook::*;

pub mod system_calls;
pub use system_calls::*;

pub mod state_changes;

pub mod calc;

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

/// Helper trait to encapsulate requirements for a type to be used as input for [`BlockExecutor`].
pub trait ExecutableTx<E: BlockExecutor + ?Sized>:
    IntoTxEnv<<E::Evm as Evm>::Tx> + RecoveredTx<E::Transaction> + Copy
{
}
impl<E: BlockExecutor, T> ExecutableTx<E> for T where
    T: IntoTxEnv<<E::Evm as Evm>::Tx> + RecoveredTx<E::Transaction> + Copy
{
}

/// A type that knows how to execute a single block.
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
    type Evm: Evm<Tx: FromRecoveredTx<Self::Transaction>>;

    /// Applies any necessary changes before executing the block's transactions.
    fn apply_pre_execution_changes(&mut self) -> Result<(), BlockExecutionError>;

    /// Executes a single transaction and applies execution result to internal state.
    ///
    /// Returns the gas used by the transaction.
    fn execute_transaction(
        &mut self,
        tx: impl ExecutableTx<Self>,
    ) -> Result<u64, BlockExecutionError> {
        self.execute_transaction_with_result_closure(tx, |_| ())
    }

    /// Executes a single transaction and applies execution result to internal state. Invokes the
    /// given closure with an internal [`ExecutionResult`] produced by the EVM.
    fn execute_transaction_with_result_closure(
        &mut self,
        tx: impl ExecutableTx<Self>,
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

    /// Executes all transactions in a block, applying pre and post execution changes.
    fn execute_block(
        mut self,
        transactions: impl IntoIterator<Item = impl ExecutableTx<Self>>,
    ) -> Result<BlockExecutionResult<Self::Receipt>, BlockExecutionError>
    where
        Self: Sized,
    {
        self.apply_pre_execution_changes()?;

        for tx in transactions {
            self.execute_transaction(tx)?;
        }

        self.apply_post_execution_changes()
    }
}

/// A helper trait encapsulating the constraints on [`BlockExecutor`] produced by the
/// [`BlockExecutorFactory`] to avoid duplicating them in every implementation.
pub trait BlockExecutorFor<'a, F: BlockExecutorFactory + ?Sized, DB, I = NoOpInspector>
where
    Self: BlockExecutor<
        Evm = <F::EvmFactory as EvmFactory>::Evm<&'a mut State<DB>, I>,
        Transaction = F::Transaction,
        Receipt = F::Receipt,
    >,
    DB: Database + 'a,
    I: Inspector<<F::EvmFactory as EvmFactory>::Context<&'a mut State<DB>>> + 'a,
{
}

impl<'a, F, DB, I, T> BlockExecutorFor<'a, F, DB, I> for T
where
    F: BlockExecutorFactory,
    DB: Database + 'a,
    I: Inspector<<F::EvmFactory as EvmFactory>::Context<&'a mut State<DB>>> + 'a,
    T: BlockExecutor<
        Evm = <F::EvmFactory as EvmFactory>::Evm<&'a mut State<DB>, I>,
        Transaction = F::Transaction,
        Receipt = F::Receipt,
    >,
{
}

/// A factory that can create [`BlockExecutor`]s.
///
/// This trait extends [`crate::EvmFactory`] and provides a way to construct a [`BlockExecutor`].
/// Executor is expected to derive most of the context for block execution from the EVM (which
/// includes [`revm::context::BlockEnv`]), and any additional context should be contained in
/// configured [`ExecutionCtx`].
///
/// Every block executor factory is expected to contain and expose an [`EvmFactory`] instance.
///
/// For more context on the executor design, see the documentation for [`BlockExecutor`].
///
/// [`ExecutionCtx`]: BlockExecutorFactory::ExecutionCtx
#[auto_impl::auto_impl(Arc)]
pub trait BlockExecutorFactory: 'static {
    /// The EVM factory used by the executor.
    type EvmFactory: EvmFactory;

    /// Context required for block execution.
    ///
    /// This is similar to [`crate::EvmEnv`], but only contains context unrelated to EVM and
    /// required for execution of an entire block.
    type ExecutionCtx<'a>: Clone;

    /// Transaction type used by the executor, see [`BlockExecutor::Transaction`].
    type Transaction;

    /// Receipt type produced by the executor, see [`BlockExecutor::Receipt`].
    type Receipt;

    /// Reference to EVM factory used by the executor.
    fn evm_factory(&self) -> &Self::EvmFactory;

    /// Creates an executor with given EVM and execution context.
    fn create_executor<'a, DB, I>(
        &'a self,
        evm: <Self::EvmFactory as EvmFactory>::Evm<&'a mut State<DB>, I>,
        ctx: Self::ExecutionCtx<'a>,
    ) -> impl BlockExecutorFor<'a, Self, DB, I>
    where
        DB: Database + 'a,
        I: Inspector<<Self::EvmFactory as EvmFactory>::Context<&'a mut State<DB>>> + 'a;
}
