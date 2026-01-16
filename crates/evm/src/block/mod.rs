//! Block execution abstraction.

use crate::{Database, Evm, EvmFactory, FromRecoveredTx, FromTxWithEncoded, RecoveredTx, ToTxEnv};
use alloc::{boxed::Box, vec::Vec};
use alloy_eips::eip7685::Requests;
use revm::{
    context::result::{ExecutionResult, ResultAndState},
    database::State,
    inspector::NoOpInspector,
    Inspector,
};

mod error;
pub use error::*;

mod state_hook;
pub use state_hook::*;

pub mod system_calls;
pub use system_calls::*;

pub mod state_changes;

pub mod state;
pub use state::*;

pub mod calc;

/// The result of executing a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockExecutionResult<T> {
    /// All the receipts of the transactions in the block.
    pub receipts: Vec<T>,
    /// All the EIP-7685 requests in the block.
    pub requests: Requests,
    /// The total gas used by the block.
    pub gas_used: u64,
    /// Blob gas used by the block.
    pub blob_gas_used: u64,
}

impl<T> Default for BlockExecutionResult<T> {
    fn default() -> Self {
        Self {
            receipts: Default::default(),
            requests: Default::default(),
            gas_used: 0,
            blob_gas_used: 0,
        }
    }
}

/// Helper trait to encapsulate requirements for a type to be used as input for [`BlockExecutor`].
///
/// This trait combines the requirements for a transaction to be executable by a block executor:
/// - Must be convertible to the EVM's transaction environment via [`ToTxEnv`]
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
    ToTxEnv<<E::Evm as Evm>::Tx> + RecoveredTx<E::Transaction>
{
}
impl<E: BlockExecutor + ?Sized, T> ExecutableTx<E> for T where
    T: ToTxEnv<<E::Evm as Evm>::Tx> + RecoveredTx<E::Transaction>
{
}

/// Marks whether transaction should be committed into block executor's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum CommitChanges {
    /// Transaction should be committed into block executor's state.
    Yes,
    /// Transaction should not be committed.
    No,
}

impl CommitChanges {
    /// Returns `true` if transaction should be committed into block executor's state.
    pub const fn should_commit(self) -> bool {
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
    /// 1. The transaction can be converted to `TxEnv` via [`ToTxEnv`] for EVM execution
    /// 2. The original transaction and signer can be accessed via [`RecoveredTx`] for receipt
    ///    generation
    ///
    /// Returns [`None`] if committing changes from the transaction should be skipped via
    /// [`CommitChanges::No`], otherwise returns the gas used by the transaction.
    fn execute_transaction_with_commit_condition(
        &mut self,
        tx: impl ExecutableTx<Self>,
        f: impl FnOnce(&ExecutionResult<<Self::Evm as Evm>::HaltReason>) -> CommitChanges,
    ) -> Result<Option<u64>, BlockExecutionError> {
        // Execute transaction without committing
        let output = self.execute_transaction_without_commit(&tx)?;

        if !f(&output.result).should_commit() {
            return Ok(None);
        }

        let gas_used = self.commit_transaction(output, tx)?;
        Ok(Some(gas_used))
    }

    /// Executes a single transaction without committing state changes.
    ///
    /// This method performs the transaction execution through the EVM but does not
    /// commit the resulting state changes. The output can be inspected and potentially
    /// committed later using [`commit_transaction`](Self::commit_transaction).
    ///
    /// Returns a [`revm::context_interface::result::ResultAndState`] containing the execution
    /// result and state changes.
    ///
    /// # Use Cases
    /// - Transaction simulation without affecting state
    /// - Inspecting transaction effects before committing
    /// - Building custom commit logic
    fn execute_transaction_without_commit(
        &mut self,
        tx: impl ExecutableTx<Self>,
    ) -> Result<ResultAndState<<Self::Evm as Evm>::HaltReason>, BlockExecutionError>;

    /// Commits a previously executed transaction's state changes.
    ///
    /// Takes the output from
    /// [`execute_transaction_without_commit`](Self::execute_transaction_without_commit)
    /// and applies the state changes, updates gas accounting, and generates a receipt.
    ///
    /// Returns the gas used by the transaction.
    ///
    /// # Parameters
    /// - `output`: The transaction output containing execution result and state changes
    /// - `tx`: The original transaction (needed for receipt generation)
    fn commit_transaction(
        &mut self,
        output: ResultAndState<<Self::Evm as Evm>::HaltReason>,
        tx: impl ExecutableTx<Self>,
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

    /// Exposes immutable reference to EVM.
    fn evm(&self) -> &Self::Evm;

    /// Returns a reference to all recorded receipts.
    fn receipts(&self) -> &[Self::Receipt];

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
/// This trait serves as the main entry point for block execution, providing a way to construct
/// [`BlockExecutor`] instances with the necessary context. It separates the concerns of:
/// - EVM configuration (handled by [`EvmFactory`])
/// - Block-specific execution context (provided via [`ExecutionCtx`])
///
/// It allows for:
/// - Reusable EVM configuration across multiple block executions
/// - Separation of EVM-related state from block execution state
/// - Flexible instantiation of executors with different contexts
///
/// # Relationship with EvmFactory
///
/// Every block executor factory contains an [`EvmFactory`] instance which handles:
/// - EVM configuration and instantiation
/// - Transaction environment setup
/// - State database management
///
/// The block executor factory extends this by adding block-level execution concerns.
///
/// For more context on the executor design, see the documentation for [`BlockExecutor`].
///
/// [`ExecutionCtx`]: BlockExecutorFactory::ExecutionCtx
/// [`EvmFactory`]: crate::EvmFactory
#[auto_impl::auto_impl(Arc)]
pub trait BlockExecutorFactory: 'static {
    /// The EVM factory used by the executor.
    type EvmFactory: EvmFactory;

    /// Context required for block execution beyond what the EVM provides (e.g.
    /// [`EvmEnv`](crate::EvmEnv))
    ///
    /// While the EVM contains transaction-level context (gas limits, caller, value) and
    /// block-level context (block number, timestamp, base fee), the `ExecutionCtx` provides
    /// additional block execution context that is specific to your consensus implementation.
    ///
    /// # Purpose
    ///
    /// This type provides data needed for system calls that occur outside normal transaction
    /// execution. Block execution requires additional context for:
    /// - **Pre-execution system calls**: Setting up block hash history, beacon block roots
    /// - **Post-execution system calls**: Applying block rewards, validator withdrawals
    /// - **Consensus-specific data**: Uncle/ommer blocks, L2 data availability info
    /// - **Protocol parameters**: Fork-specific rules, precompile configurations
    /// - **Precompile metadata**: Context for precompiles that require block-level data (e.g.
    ///   parameters stored in the block body)
    ///
    /// For example, in Ethereum: [`EthBlockExecutionCtx`](crate::eth::EthBlockExecutionCtx)
    /// contains:
    /// - Parent block hash for EIP-2935 block hash system call
    /// - Parent beacon block root for EIP-4788 beacon root system call
    /// - Uncle blocks for handling uncle rewards
    /// - Withdrawals for EIP-4895 validator withdrawals
    ///
    /// # Design Considerations
    ///
    /// - Must be [`Clone`] to support creating multiple executors, can use `Cow` borrowed from the
    ///   block.
    /// - Should be lightweight (use references where possible)
    /// - Contains only block-level data, not transaction-specific data
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
    ///
    /// This method combines:
    /// - An EVM instance (already configured with block environment and state)
    /// - The execution context (containing additional data for system calls)
    ///
    /// To create a [`BlockExecutor`] that can:
    /// 1. Apply pre-execution system calls (e.g., EIP-2935 blockhashes, EIP-4788 beacon roots)
    /// 2. Execute transactions
    /// 3. Apply post-execution system calls (e.g., withdrawals, rewards)
    ///
    /// # Parameters
    ///
    /// - `evm`: A configured EVM instance with block environment and state
    /// - `ctx`: The execution context containing consensus-specific data needed for system calls
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Create EVM with block environment
    /// let evm = factory.evm_factory().create_evm(block_env, state_db, inspector);
    ///
    /// // Create execution context with consensus-specific data required for block execution
    /// let ctx = EthBlockExecutionCtx {
    ///     parent_hash: parent_block.hash(),
    ///     parent_beacon_block_root: parent_block.parent_beacon_block_root,
    ///     ommers: &uncle_blocks,
    ///     withdrawals: Some(Cow::Borrowed(&withdrawals)),
    /// };
    ///
    /// // Create executor - it will use ctx for system calls
    /// let mut executor = factory.create_executor(evm, ctx);
    ///
    /// // The executor will:
    /// // 1. Apply pre-execution changes
    /// // 2. Execute all transactions
    /// // 3. Apply post-execution changes (e.g., process withdrawals, apply rewards)
    /// let result = executor.execute_block(transactions)?;
    /// ```
    fn create_executor<'a, DB, I>(
        &'a self,
        evm: <Self::EvmFactory as EvmFactory>::Evm<&'a mut State<DB>, I>,
        ctx: Self::ExecutionCtx<'a>,
    ) -> impl BlockExecutorFor<'a, Self, DB, I>
    where
        DB: Database + 'a,
        I: Inspector<<Self::EvmFactory as EvmFactory>::Context<&'a mut State<DB>>> + 'a;
}
