//! Block execution abstraction.

use crate::{
    Database, Evm, EvmFactory, FromRecoveredTx, FromTxWithEncoded, IntoTxEnv, RecoveredTx,
};
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
///
/// This trait combines the requirements for a transaction to be executable by a block executor:
/// - Must be convertible to the EVM's transaction environment via [`IntoTxEnv`]
/// - Must provide access to the transaction and signer via [`RecoveredTx`]
/// - Must be [`Copy`] for efficient handling during block execution (the expectation here is that
///   this always passed as & reference)
///
/// This trait is automatically implemented for any type that meets these requirements.
/// Common implementations include:
/// - [`Recovered<T>`](alloy_consensus::transaction::Recovered) where `T` is a transaction type
/// - [`WithEncoded<Recovered<T>>`](alloy_eips::eip2718::WithEncoded) for transactions with encoded
///   bytes
///
/// The trait ensures that the block executor can both execute the transaction in the EVM
/// and access the original transaction data for receipt generation.
pub trait ExecutableTx<E: BlockExecutor + ?Sized>:
    IntoTxEnv<<E::Evm as Evm>::Tx> + RecoveredTx<E::Transaction> + Copy
{
}
impl<E: BlockExecutor, T> ExecutableTx<E> for T where
    T: IntoTxEnv<<E::Evm as Evm>::Tx> + RecoveredTx<E::Transaction> + Copy
{
}

/// Marks whether transaction should be commited into block executor's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum CommitChanges {
    /// Transaction should be commited into block executor's state.
    Yes,
    /// Transaction should not be commited.
    No,
}

impl CommitChanges {
    /// Returns `true` if transaction should be commited into block executor's state.
    pub fn should_commit(self) -> bool {
        matches!(self, Self::Yes)
    }
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
    ///
    /// This represents the consensus transaction type that the block executor operates on.
    /// It's typically a type from the consensus layer (e.g.,
    /// [`EthereumTxEnvelope`](alloy_consensus::EthereumTxEnvelope)) that contains
    /// the raw transaction data, signature, and other consensus-level information.
    ///
    /// This type is used in several contexts:
    /// - As the generic parameter for [`RecoveredTx<T>`](crate::RecoveredTx) in [`ExecutableTx`]
    /// - As the generic parameter for [`FromRecoveredTx<T>`](crate::FromRecoveredTx) and
    ///   [`FromTxWithEncoded<T>`](crate::FromTxWithEncoded) in the EVM constraint
    /// - To generate receipts after transaction execution
    ///
    /// The transaction flow is:
    /// 1. `Self::Transaction` (consensus tx) →
    ///    [`Recovered<Self::Transaction>`](alloy_consensus::transaction::Recovered) (with sender)
    /// 2. [`Recovered<Self::Transaction>`](alloy_consensus::transaction::Recovered) →
    ///    [`TxEnv`](revm::context::TxEnv) (via [`FromRecoveredTx`])
    /// 3. [`TxEnv`](revm::context::TxEnv) → EVM execution → [`ExecutionResult`]
    /// 4. [`ExecutionResult`] + `Self::Transaction` → `Self::Receipt`
    ///
    /// Common examples:
    /// - [`EthereumTxEnvelope`](alloy_consensus::EthereumTxEnvelope) for all Ethereum transaction
    ///   variants
    /// - `OpTxEnvelope` for opstack transaction variants
    type Transaction;
    /// Receipt type this executor produces.
    type Receipt;
    /// EVM used by the executor.
    ///
    /// The EVM's transaction type (`Evm::Tx`) must be able to be constructed from both:
    /// - [`FromRecoveredTx<Self::Transaction>`](crate::FromRecoveredTx) - for transactions with
    ///   recovered senders
    /// - [`FromTxWithEncoded<Self::Transaction>`](crate::FromTxWithEncoded) - for transactions with
    ///   encoded bytes
    ///
    /// This constraint ensures that the block executor can convert consensus transactions
    /// into the EVM's transaction format for execution.
    type Evm: Evm<Tx: FromRecoveredTx<Self::Transaction> + FromTxWithEncoded<Self::Transaction>>;

    /// Applies any necessary changes before executing the block's transactions.
    fn apply_pre_execution_changes(&mut self) -> Result<(), BlockExecutionError>;

    /// Executes a single transaction and applies execution result to internal state.
    ///
    /// This method accepts any type implementing [`ExecutableTx`], which ensures the transaction:
    /// - Can be converted to the EVM's transaction environment for execution
    /// - Provides access to the original transaction and signer for receipt generation
    ///
    /// Common input types include:
    /// - `&Recovered<Transaction>` - A transaction with its recovered sender
    /// - `&WithEncoded<Recovered<Transaction>>` - A transaction with sender and encoded bytes
    ///
    /// The transaction is executed in the EVM, state changes are committed, and a receipt
    /// is generated internally.
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
    ///
    /// This method is similar to [`execute_transaction`](Self::execute_transaction) but provides
    /// access to the raw execution result before it's converted to a receipt. This is useful for:
    /// - Custom logging or metrics collection
    /// - Debugging transaction execution
    /// - Extracting additional information from the execution result
    ///
    /// The transaction is always committed after the closure is invoked.
    fn execute_transaction_with_result_closure(
        &mut self,
        tx: impl ExecutableTx<Self>,
        f: impl FnOnce(&ExecutionResult<<Self::Evm as Evm>::HaltReason>),
    ) -> Result<u64, BlockExecutionError> {
        self.execute_transaction_with_commit_condition(tx, |res| {
            f(res);
            CommitChanges::Yes
        })
        .map(Option::unwrap_or_default)
    }

    /// Executes a single transaction and applies execution result to internal state. Invokes the
    /// given closure with an internal [`ExecutionResult`] produced by the EVM, and commits the
    /// transaction to the state on [`CommitChanges::Yes`].
    ///
    /// This is the most flexible transaction execution method, allowing conditional commitment
    /// based on the execution result. The closure receives the execution result and returns
    /// whether to commit the changes to state.
    ///
    /// Use cases:
    /// - Conditional execution based on transaction outcome
    /// - Simulating transactions without committing
    /// - Custom validation logic before committing
    ///
    /// The [`ExecutableTx`] constraint ensures that:
    /// 1. The transaction can be converted to `TxEnv` via [`IntoTxEnv`] for EVM execution
    /// 2. The original transaction and signer can be accessed via [`RecoveredTx`] for receipt
    ///    generation
    ///
    /// Returns [`None`] if commiting changes from the transaction should be skipped via
    /// [`CommitChanges::No`], otherwise returns the gas used by the transaction.
    fn execute_transaction_with_commit_condition(
        &mut self,
        tx: impl ExecutableTx<Self>,
        f: impl FnOnce(&ExecutionResult<<Self::Evm as Evm>::HaltReason>) -> CommitChanges,
    ) -> Result<Option<u64>, BlockExecutionError>;

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

    /// Exposes immutable reference to EVM.
    fn evm(&self) -> &Self::Evm;

    /// Executes all transactions in a block, applying pre and post execution changes.
    ///
    /// This is a convenience method that orchestrates the complete block execution flow:
    /// 1. Applies pre-execution changes (system calls, irregular state transitions)
    /// 2. Executes all transactions in order
    /// 3. Applies post-execution changes (block rewards, system calls)
    ///
    /// Each transaction in the iterator must implement [`ExecutableTx`], ensuring it can be:
    /// - Converted to the EVM's transaction format for execution
    /// - Used to generate receipts with access to the original transaction data
    ///
    /// # Example
    ///
    /// ```ignore
    /// let recovered_txs: Vec<Recovered<Transaction>> = block.transactions
    ///     .iter()
    ///     .map(|tx| tx.recover_signer())
    ///     .collect::<Result<_, _>>()?;
    ///
    /// let result = executor.execute_block(recovered_txs.iter())?;
    /// ```
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
    ///
    /// This should be the same consensus transaction type that the block executor operates on.
    /// It represents the transaction format from your consensus layer that needs to be
    /// executed by the EVM.
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
