//! Abstraction over EVM.

use crate::{EvmEnv, EvmError, IntoTxEnv};
use alloy_primitives::{Address, Bytes};
use core::{error::Error, fmt::Debug, hash::Hash};
use revm::{
    context::{result::ExecutionResult, BlockEnv},
    context_interface::{
        result::{HaltReasonTr, ResultAndState},
        ContextTr,
    },
    inspector::{JournalExt, NoOpInspector},
    DatabaseCommit, Inspector,
};

/// Helper trait to bound [`revm::Database::Error`] with common requirements.
pub trait Database: revm::Database<Error: Error + Send + Sync + 'static> {}
impl<T> Database for T where T: revm::Database<Error: Error + Send + Sync + 'static> {}

/// An instance of an ethereum virtual machine.
///
/// An EVM is commonly initialized with the corresponding block context and state and it's only
/// purpose is to execute transactions.
///
/// Executing a transaction will return the outcome of the transaction.
pub trait Evm {
    /// Database type held by the EVM.
    type DB;
    /// The transaction object that the EVM will execute.
    ///
    /// Implementations are expected to rely on a single entrypoint for transaction execution such
    /// as [`revm::context::TxEnv`]. The actual set of valid inputs is not limited by allowing to
    /// provide any [`IntoTxEnv`] implementation for [`Evm::transact`] method.
    type Tx: IntoTxEnv<Self::Tx>;
    /// Error type returned by EVM. Contains either errors related to invalid transactions or
    /// internal irrecoverable execution errors.
    type Error: EvmError;
    /// Halt reason. Enum over all possible reasons for halting the execution. When execution halts,
    /// it means that transaction is valid, however, it's execution was interrupted (e.g because of
    /// running out of gas or overflowing stack).
    type HaltReason: HaltReasonTr + Send + Sync + 'static;
    /// Identifier of the EVM specification. EVM is expected to use this identifier to determine
    /// which features are enabled.
    type Spec: Debug + Copy + Hash + Eq + Send + Sync + Default + 'static;
    /// Precompiles used by the EVM.
    type Precompiles;
    /// Evm inspector.
    type Inspector;

    /// Reference to [`BlockEnv`].
    fn block(&self) -> &BlockEnv;

    /// Returns the chain ID of the environment.
    fn chain_id(&self) -> u64;

    /// Executes a transaction and returns the outcome.
    fn transact_raw(
        &mut self,
        tx: Self::Tx,
    ) -> Result<ResultAndState<Self::HaltReason>, Self::Error>;

    /// Same as [`Evm::transact_raw`], but takes a [`IntoTxEnv`] implementation, thus allowing to
    /// support transacting with an external type.
    fn transact(
        &mut self,
        tx: impl IntoTxEnv<Self::Tx>,
    ) -> Result<ResultAndState<Self::HaltReason>, Self::Error> {
        self.transact_raw(tx.into_tx_env())
    }

    /// Executes a system call.
    ///
    /// Note: this will only keep the target `contract` in the state. This is done because revm is
    /// loading [`BlockEnv::beneficiary`] into state by default, and we need to avoid it by also
    /// covering edge cases when beneficiary is set to the system contract address.
    fn transact_system_call(
        &mut self,
        caller: Address,
        contract: Address,
        data: Bytes,
    ) -> Result<ResultAndState<Self::HaltReason>, Self::Error>;

    /// Returns a mutable reference to the underlying database.
    fn db_mut(&mut self) -> &mut Self::DB;

    /// Executes a transaction and commits the state changes to the underlying database.
    fn transact_commit(
        &mut self,
        tx: impl IntoTxEnv<Self::Tx>,
    ) -> Result<ExecutionResult<Self::HaltReason>, Self::Error>
    where
        Self::DB: DatabaseCommit,
    {
        let ResultAndState { result, state } = self.transact(tx)?;
        self.db_mut().commit(state);

        Ok(result)
    }

    /// Consumes the EVM and returns the inner [`EvmEnv`].
    fn finish(self) -> (Self::DB, EvmEnv<Self::Spec>)
    where
        Self: Sized;

    /// Consumes the EVM and returns the inner database.
    fn into_db(self) -> Self::DB
    where
        Self: Sized,
    {
        self.finish().0
    }

    /// Consumes the EVM and returns the inner [`EvmEnv`].
    fn into_env(self) -> EvmEnv<Self::Spec>
    where
        Self: Sized,
    {
        self.finish().1
    }

    /// Determines whether additional transactions should be inspected or not.
    ///
    /// See also [`EvmFactory::create_evm_with_inspector`].
    fn set_inspector_enabled(&mut self, enabled: bool);

    /// Enables the configured inspector.
    ///
    /// All additional transactions will be inspected if enabled.
    fn enable_inspector(&mut self) {
        self.set_inspector_enabled(true)
    }

    /// Disables the configured inspector.
    ///
    /// Transactions will no longer be inspected.
    fn disable_inspector(&mut self) {
        self.set_inspector_enabled(false)
    }

    /// Getter of precompiles.
    fn precompiles(&self) -> &Self::Precompiles;

    /// Mutable getter of precompiles.
    fn precompiles_mut(&mut self) -> &mut Self::Precompiles;

    /// Getter of inspector.
    fn inspector(&self) -> &Self::Inspector;

    /// Mutable getter of inspector.
    fn inspector_mut(&mut self) -> &mut Self::Inspector;
}

/// A type responsible for creating instances of an ethereum virtual machine given a certain input.
pub trait EvmFactory {
    /// The EVM type that this factory creates.
    type Evm<DB: Database, I: Inspector<Self::Context<DB>>>: Evm<
        DB = DB,
        Tx = Self::Tx,
        HaltReason = Self::HaltReason,
        Error = Self::Error<DB::Error>,
        Spec = Self::Spec,
        Precompiles = Self::Precompiles,
        Inspector = I,
    >;

    /// The EVM context for inspectors
    type Context<DB: Database>: ContextTr<Db = DB, Journal: JournalExt>;
    /// Transaction environment.
    type Tx: IntoTxEnv<Self::Tx>;
    /// EVM error. See [`Evm::Error`].
    type Error<DBError: Error + Send + Sync + 'static>: EvmError;
    /// Halt reason. See [`Evm::HaltReason`].
    type HaltReason: HaltReasonTr + Send + Sync + 'static;
    /// The EVM specification identifier, see [`Evm::Spec`].
    type Spec: Debug + Copy + Hash + Eq + Send + Sync + Default + 'static;
    /// Precompiles used by the EVM.
    type Precompiles;

    /// Creates a new instance of an EVM.
    fn create_evm<DB: Database>(
        &self,
        db: DB,
        evm_env: EvmEnv<Self::Spec>,
    ) -> Self::Evm<DB, NoOpInspector>;

    /// Creates a new instance of an EVM with an inspector.
    ///
    /// Note: It is expected that the [`Inspector`] is usually provided as `&mut Inspector` so that
    /// it remains owned by the call site when [`Evm::transact`] is invoked.
    fn create_evm_with_inspector<DB: Database, I: Inspector<Self::Context<DB>>>(
        &self,
        db: DB,
        input: EvmEnv<Self::Spec>,
        inspector: I,
    ) -> Self::Evm<DB, I>;
}
