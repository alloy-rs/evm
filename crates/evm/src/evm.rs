//! Abstraction over EVM.

use crate::{EvmError, IntoTxEnv};
use alloy_primitives::{Address, Bytes};
use core::error::Error;
use revm::{
    context::BlockEnv,
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
    type Tx;
    /// Error type returned by EVM. Contains either errors related to invalid transactions or
    /// internal irrecoverable execution errors.
    type Error: EvmError;
    /// Halt reason. Enum over all possible reasons for halting the execution. When execution halts,
    /// it means that transaction is valid, however, it's execution was interrupted (e.g because of
    /// running out of gas or overflowing stack).
    type HaltReason: HaltReasonTr + Send + Sync + 'static;

    /// Reference to [`BlockEnv`].
    fn block(&self) -> &BlockEnv;

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
    ) -> Result<ResultAndState<Self::HaltReason>, Self::Error>
    where
        Self::DB: DatabaseCommit,
    {
        let result = self.transact(tx)?;
        self.db_mut().commit(result.state.clone());

        Ok(result)
    }
}

/// A type responsible for creating instances of an ethereum virtual machine given a certain input.
pub trait EvmFactory<Input> {
    /// The EVM type that this factory creates.
    // TODO: this doesn't quite work because this would force use to use an enum approach for trace
    // evm for example, unless we
    type Evm<DB: Database, I: Inspector<Self::Context<DB>>>: Evm<
        DB = DB,
        Tx = Self::Tx,
        HaltReason = Self::HaltReason,
        Error = Self::Error<DB::Error>,
    >;

    /// The EVM context for inspectors
    type Context<DB: Database>: ContextTr<Db = DB, Journal: JournalExt>;
    /// Transaction environment.
    type Tx;
    /// EVM error. See [`Evm::Error`].
    type Error<DBError: Error + Send + Sync + 'static>: EvmError;
    /// Halt reason. See [`Evm::HaltReason`].
    type HaltReason: HaltReasonTr + Send + Sync + 'static;

    /// Creates a new instance of an EVM.
    fn create_evm<DB: Database>(&self, db: DB, input: Input) -> Self::Evm<DB, NoOpInspector>;

    /// Creates a new instance of an EVM with an inspector.
    fn create_evm_with_inspector<DB: Database, I: Inspector<Self::Context<DB>>>(
        &self,
        db: DB,
        input: Input,
        inspector: I,
    ) -> Self::Evm<DB, I>;
}
