//! EVM traits.

use crate::Database;
use alloc::boxed::Box;
use alloy_primitives::{Address, Log, B256, U256};
use core::{error::Error, fmt, fmt::Debug};
use revm::{
    context::{journaled_state::TransferError, Block, DBErrorMarker, JournalTr},
    interpreter::{SStoreResult, StateLoad},
    primitives::{StorageKey, StorageValue},
    state::{Account, AccountInfo, Bytecode},
};

/// Erased error type.
#[derive(thiserror::Error, Debug)]
#[error(transparent)]
pub struct ErasedError(Box<dyn Error + Send + Sync + 'static>);

impl ErasedError {
    /// Creates a new [`ErasedError`].
    pub fn new(error: impl Error + Send + Sync + 'static) -> Self {
        Self(Box::new(error))
    }
}

impl DBErrorMarker for ErasedError {}

/// Errors returned by [`EvmInternals`].
#[derive(Debug, thiserror::Error)]
pub enum EvmInternalsError {
    /// Database error.
    #[error(transparent)]
    Database(ErasedError),
}

impl EvmInternalsError {
    /// Creates a new [`EvmInternalsError::Database`]
    pub fn database(err: impl Error + Send + Sync + 'static) -> Self {
        Self::Database(ErasedError::new(err))
    }
}

/// dyn-compatible trait for accessing and modifying EVM internals, particularly the journal.
///
/// This trait provides an abstraction over journal operations without exposing
/// associated types, making it object-safe and suitable for dynamic dispatch.
trait EvmInternalsTr: Database<Error = ErasedError> + Debug {
    fn load_account(&mut self, address: Address) -> Result<StateLoad<&Account>, EvmInternalsError>;

    fn load_account_code(
        &mut self,
        address: Address,
    ) -> Result<StateLoad<&Account>, EvmInternalsError>;

    /// Increments the balance of the account.
    fn balance_incr(&mut self, address: Address, balance: U256) -> Result<(), EvmInternalsError>;

    /// Sets the balance of the the account
    ///
    /// Touches the account in all cases.
    ///
    /// If the given `balance` is the same as the account's, no journal entry is created.
    fn set_balance(&mut self, address: Address, balance: U256) -> Result<(), EvmInternalsError>;

    /// Transfers the balance from one account to another.
    ///
    /// This will load both accounts
    fn transfer(
        &mut self,
        from: Address,
        to: Address,
        balance: U256,
    ) -> Result<Option<TransferError>, EvmInternalsError>;

    /// Increments the nonce of the account.
    ///
    /// This creates a new journal entry with this change.
    fn bump_nonce(&mut self, address: Address) -> Result<(), EvmInternalsError>;

    fn sload(
        &mut self,
        address: Address,
        key: StorageKey,
    ) -> Result<StateLoad<StorageValue>, EvmInternalsError>;

    fn touch_account(&mut self, address: Address);

    fn set_code(&mut self, address: Address, code: Bytecode);

    /// Sets bytecode with hash. Assume that account is warm.
    fn set_code_with_hash(&mut self, address: Address, code: Bytecode, hash: B256);

    fn sstore(
        &mut self,
        address: Address,
        key: StorageKey,
        value: StorageValue,
    ) -> Result<StateLoad<SStoreResult>, EvmInternalsError>;

    fn log(&mut self, log: Log);

    fn tload(&mut self, address: Address, key: StorageKey) -> StorageValue;

    fn tstore(&mut self, address: Address, key: StorageKey, value: StorageValue);
}

/// Helper internal struct for implementing [`EvmInternals`].
#[derive(Debug)]
struct EvmInternalsImpl<'a, T>(&'a mut T);

impl<T> revm::Database for EvmInternalsImpl<'_, T>
where
    T: JournalTr<Database: Database>,
{
    type Error = ErasedError;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.0.db_mut().basic(address).map_err(ErasedError::new)
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        self.0.db_mut().code_by_hash(code_hash).map_err(ErasedError::new)
    }

    fn storage(
        &mut self,
        address: Address,
        index: StorageKey,
    ) -> Result<StorageValue, Self::Error> {
        self.0.db_mut().storage(address, index).map_err(ErasedError::new)
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        self.0.db_mut().block_hash(number).map_err(ErasedError::new)
    }
}

impl<T> EvmInternalsTr for EvmInternalsImpl<'_, T>
where
    T: JournalTr<Database: Database> + Debug,
{
    fn load_account(&mut self, address: Address) -> Result<StateLoad<&Account>, EvmInternalsError> {
        self.0.load_account(address).map_err(EvmInternalsError::database)
    }

    fn load_account_code(
        &mut self,
        address: Address,
    ) -> Result<StateLoad<&Account>, EvmInternalsError> {
        self.0.load_account_with_code(address).map_err(EvmInternalsError::database)
    }

    fn balance_incr(&mut self, address: Address, balance: U256) -> Result<(), EvmInternalsError> {
        self.0.balance_incr(address, balance).map_err(EvmInternalsError::database)
    }

    fn set_balance(&mut self, address: Address, balance: U256) -> Result<(), EvmInternalsError> {
        let mut account = self.0.load_account_mut(address).map_err(EvmInternalsError::database)?;
        account.set_balance(balance);
        Ok(())
    }

    fn transfer(
        &mut self,
        from: Address,
        to: Address,
        balance: U256,
    ) -> Result<Option<TransferError>, EvmInternalsError> {
        self.0.transfer(from, to, balance).map_err(EvmInternalsError::database)
    }

    fn bump_nonce(&mut self, address: Address) -> Result<(), EvmInternalsError> {
        self.0.load_account_mut(address).map_err(EvmInternalsError::database)?.bump_nonce();
        Ok(())
    }

    fn sload(
        &mut self,
        address: Address,
        key: StorageKey,
    ) -> Result<StateLoad<StorageValue>, EvmInternalsError> {
        self.0.sload(address, key).map_err(EvmInternalsError::database)
    }

    fn touch_account(&mut self, address: Address) {
        self.0.touch_account(address);
    }

    fn set_code(&mut self, address: Address, code: Bytecode) {
        self.0.set_code(address, code);
    }

    fn set_code_with_hash(&mut self, address: Address, code: Bytecode, hash: B256) {
        self.0.set_code_with_hash(address, code, hash);
    }

    fn sstore(
        &mut self,
        address: Address,
        key: StorageKey,
        value: StorageValue,
    ) -> Result<StateLoad<SStoreResult>, EvmInternalsError> {
        self.0.sstore(address, key, value).map_err(EvmInternalsError::database)
    }

    fn log(&mut self, log: Log) {
        self.0.log(log);
    }

    fn tload(&mut self, address: Address, key: StorageKey) -> StorageValue {
        self.0.tload(address, key)
    }

    fn tstore(&mut self, address: Address, key: StorageKey, value: StorageValue) {
        self.0.tstore(address, key, value);
    }
}

/// Helper type exposing hooks into EVM and access to evm internal settings.
pub struct EvmInternals<'a> {
    internals: Box<dyn EvmInternalsTr + 'a>,
    block_env: &'a (dyn Block + 'a),
}

impl<'a> EvmInternals<'a> {
    /// Creates a new [`EvmInternals`] instance.
    pub fn new<T>(journal: &'a mut T, block_env: &'a dyn Block) -> Self
    where
        T: JournalTr<Database: Database> + Debug,
    {
        Self { internals: Box::new(EvmInternalsImpl(journal)), block_env }
    }

    /// Returns the  evm's block information.
    pub const fn block_env(&self) -> impl Block + 'a {
        self.block_env
    }

    /// Returns the current block number.
    pub fn block_number(&self) -> U256 {
        self.block_env.number()
    }

    /// Returns the current block timestamp.
    pub fn block_timestamp(&self) -> U256 {
        self.block_env.timestamp()
    }

    /// Returns a mutable reference to [`Database`] implementation with erased error type.
    ///
    /// Users should prefer using other methods for accessing state that rely on cached state in the
    /// journal instead.
    pub fn db_mut(&mut self) -> impl Database<Error = ErasedError> + '_ {
        &mut *self.internals
    }

    /// Loads an account.
    pub fn load_account(
        &mut self,
        address: Address,
    ) -> Result<StateLoad<&Account>, EvmInternalsError> {
        self.internals.load_account(address)
    }

    /// Loads an account AND it's code.
    pub fn load_account_code(
        &mut self,
        address: Address,
    ) -> Result<StateLoad<&Account>, EvmInternalsError> {
        self.internals.load_account_code(address)
    }

    /// Increments the balance of the account.
    pub fn balance_incr(
        &mut self,
        address: Address,
        balance: U256,
    ) -> Result<(), EvmInternalsError> {
        self.internals.balance_incr(address, balance)
    }

    /// Sets the balance of the the account
    ///
    /// Touches the account in all cases.
    ///
    /// If the given `balance` is the same as the account's, no journal entry is created.
    pub fn set_balance(
        &mut self,
        address: Address,
        balance: U256,
    ) -> Result<(), EvmInternalsError> {
        self.internals.set_balance(address, balance)
    }

    /// Transfers the balance from one account to another.
    ///
    /// This will load both accounts and return an error if the transfer fails.
    pub fn transfer(
        &mut self,
        from: Address,
        to: Address,
        balance: U256,
    ) -> Result<Option<TransferError>, EvmInternalsError> {
        self.internals.transfer(from, to, balance)
    }

    /// Increments the nonce of the account.
    ///
    /// This creates a new journal entry with this change.
    pub fn bump_nonce(&mut self, address: Address) -> Result<(), EvmInternalsError> {
        self.internals.bump_nonce(address)
    }

    /// Loads a storage slot.
    pub fn sload(
        &mut self,
        address: Address,
        key: StorageKey,
    ) -> Result<StateLoad<StorageValue>, EvmInternalsError> {
        self.internals.sload(address, key)
    }

    /// Touches the account.
    pub fn touch_account(&mut self, address: Address) {
        self.internals.touch_account(address);
    }

    /// Sets bytecode to the account.
    pub fn set_code(&mut self, address: Address, code: Bytecode) {
        self.internals.set_code(address, code);
    }

    /// Sets bytecode with hash to the account.
    ///
    /// Assumes that the account is warm.
    pub fn set_code_with_hash(&mut self, address: Address, code: Bytecode, hash: B256) {
        self.internals.set_code_with_hash(address, code, hash);
    }

    /// Stores the storage value in Journal state.
    pub fn sstore(
        &mut self,
        address: Address,
        key: StorageKey,
        value: StorageValue,
    ) -> Result<StateLoad<SStoreResult>, EvmInternalsError> {
        self.internals.sstore(address, key, value)
    }

    /// Logs the log in Journal state.
    pub fn log(&mut self, log: Log) {
        self.internals.log(log);
    }

    /// Loads a transient storage value.
    pub fn tload(&mut self, address: Address, key: StorageKey) -> StorageValue {
        self.internals.tload(address, key)
    }

    /// Stores a transient storage value.
    pub fn tstore(&mut self, address: Address, key: StorageKey, value: StorageValue) {
        self.internals.tstore(address, key, value);
    }
}

impl<'a> fmt::Debug for EvmInternals<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EvmInternals")
            .field("internals", &self.internals)
            .field("block_env", &"{{}}")
            .finish_non_exhaustive()
    }
}
