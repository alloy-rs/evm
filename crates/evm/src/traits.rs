//! EVM traits.

use crate::Database;
use alloc::boxed::Box;
use alloy_primitives::{Address, Log, B256, U256};
use core::{error::Error, fmt, fmt::Debug};
use revm::{
    context::{
        journaled_state::{account::JournaledAccountTr, TransferError},
        Block, Cfg, ContextTr, DBErrorMarker, JournalTr, Transaction,
    },
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

    fn load_account_mut_skip_cold_load<'a>(
        &'a mut self,
        address: Address,
        skip_cold_load: bool,
    ) -> Result<StateLoad<Box<dyn JournaledAccountTr + 'a>>, EvmInternalsError>;

    /// Loads an account mutably.
    fn load_account_mut<'a>(
        &'a mut self,
        address: Address,
    ) -> Result<StateLoad<Box<dyn JournaledAccountTr + 'a>>, EvmInternalsError> {
        self.load_account_mut_skip_cold_load(address, false)
    }

    fn load_account_code<'a>(
        &'a mut self,
        address: Address,
    ) -> Result<StateLoad<Box<dyn JournaledAccountTr + 'a>>, EvmInternalsError> {
        let mut account = self.load_account_mut(address)?;
        account.load_code().map_err(|e| EvmInternalsError::database(e.unwrap_db_error()))?;
        Ok(account)
    }

    /// Increments the balance of the account.
    fn balance_incr(&mut self, address: Address, balance: U256) -> Result<(), EvmInternalsError> {
        self.load_account_mut(address)?.incr_balance(balance);
        Ok(())
    }

    /// Sets the balance of the the account
    ///
    /// Touches the account in all cases.
    ///
    /// If the given `balance` is the same as the account's, no journal entry is created.
    fn set_balance(&mut self, address: Address, balance: U256) -> Result<(), EvmInternalsError> {
        self.load_account_mut(address)?.set_balance(balance);
        Ok(())
    }

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
    fn bump_nonce(&mut self, address: Address) -> Result<(), EvmInternalsError> {
        self.load_account_mut(address)?.bump_nonce();
        Ok(())
    }

    fn sload(
        &mut self,
        address: Address,
        key: StorageKey,
    ) -> Result<StateLoad<StorageValue>, EvmInternalsError> {
        self.load_account_mut(address)?
            .sload(key, false)
            .map(|i| i.map(|i| i.present_value()))
            .map_err(|e| EvmInternalsError::database(e.unwrap_db_error()))
    }

    fn touch_account(&mut self, address: Address) -> Result<(), EvmInternalsError> {
        self.load_account_mut(address)?.touch();
        Ok(())
    }

    /// Sets bytecode to the account. Internally calls [`EvmInternalsTr::set_code_with_hash`].
    ///
    /// This will load the account, mark it as touched and set the code and code hash.
    /// It will return an error if database error occurs.
    fn set_code(&mut self, address: Address, code: Bytecode) -> Result<(), EvmInternalsError> {
        let hash = code.hash_slow();
        self.set_code_with_hash(address, code, hash)
    }

    /// Sets bytecode with hash to the account.
    ///
    /// This will load the account, mark it as touched and set the code and code hash.
    /// It will return an error if database error occurs.
    fn set_code_with_hash(
        &mut self,
        address: Address,
        code: Bytecode,
        hash: B256,
    ) -> Result<(), EvmInternalsError> {
        self.load_account_mut(address)?.set_code(hash, code);
        Ok(())
    }

    fn sstore(
        &mut self,
        address: Address,
        key: StorageKey,
        value: StorageValue,
    ) -> Result<StateLoad<SStoreResult>, EvmInternalsError> {
        self.load_account_mut(address)?
            .sstore(key, value, false)
            .map_err(|e| EvmInternalsError::database(e.unwrap_db_error()))
    }

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

    fn load_account_mut_skip_cold_load<'a>(
        &'a mut self,
        address: Address,
        skip_cold_load: bool,
    ) -> Result<StateLoad<Box<dyn JournaledAccountTr + 'a>>, EvmInternalsError> {
        self.0
            .load_account_mut_skip_cold_load(address, skip_cold_load)
            .map(|state_load| {
                state_load.map(|journaled_account| {
                    Box::new(journaled_account) as Box<dyn JournaledAccountTr + 'a>
                })
            })
            .map_err(EvmInternalsError::database)
    }

    fn transfer(
        &mut self,
        from: Address,
        to: Address,
        balance: U256,
    ) -> Result<Option<TransferError>, EvmInternalsError> {
        self.0.transfer(from, to, balance).map_err(EvmInternalsError::database)
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
    chain_id: u64,
    tx_origin: Address,
}

impl<'a> EvmInternals<'a> {
    /// Creates a new [`EvmInternals`] instance.
    pub fn new<T>(
        journal: &'a mut T,
        block_env: &'a dyn Block,
        cfg_env: &'a impl Cfg,
        tx_env: &'a impl Transaction,
    ) -> Self
    where
        T: JournalTr<Database: Database> + Debug,
    {
        Self {
            internals: Box::new(EvmInternalsImpl(journal)),
            block_env,
            chain_id: cfg_env.chain_id(),
            tx_origin: tx_env.caller(),
        }
    }

    /// Creates a new [`EvmInternals`] instance from a [`ContextTr`].
    pub fn from_context<CTX>(ctx: &'a mut CTX) -> Self
    where
        CTX: ContextTr<Journal: JournalTr<Database: Database> + Debug>,
    {
        let (block, tx, cfg, journaled_state, ..) = ctx.all_mut();
        Self::new(journaled_state, block, cfg, tx)
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

    /// Returns the chain ID.
    pub const fn chain_id(&self) -> u64 {
        self.chain_id
    }

    /// Returns the caller of the top-level call.
    ///
    /// Note that this might be different from the caller of the specific precompile call.
    pub const fn tx_origin(&self) -> Address {
        self.tx_origin
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

    /// Loads an account.
    pub fn load_account_mut<'b>(
        &'b mut self,
        address: Address,
    ) -> Result<StateLoad<Box<dyn JournaledAccountTr + 'b>>, EvmInternalsError> {
        self.internals.load_account_mut(address)
    }

    /// Loads an account mutably, skipping cold load if specified.
    pub fn load_account_mut_skip_cold_load<'b>(
        &'b mut self,
        address: Address,
        skip_cold_load: bool,
    ) -> Result<StateLoad<Box<dyn JournaledAccountTr + 'b>>, EvmInternalsError> {
        self.internals.load_account_mut_skip_cold_load(address, skip_cold_load)
    }

    /// Loads an account AND it's code.
    pub fn load_account_code<'b>(
        &'b mut self,
        address: Address,
    ) -> Result<StateLoad<Box<dyn JournaledAccountTr + 'b>>, EvmInternalsError> {
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
    ///
    /// This will load the account and return an error if database error occurs.
    pub fn touch_account(&mut self, address: Address) -> Result<(), EvmInternalsError> {
        self.internals.touch_account(address)
    }

    /// Sets bytecode to the account.
    ///
    /// This will load the account, mark it as touched and set the code and code hash.
    /// It will return an error if database error occurs.
    pub fn set_code(&mut self, address: Address, code: Bytecode) -> Result<(), EvmInternalsError> {
        self.internals.set_code(address, code)
    }

    /// Sets bytecode with hash to the account.
    ///
    /// This will load the account, mark it as touched and set the code and code hash.
    /// It will return an error if database error occurs.
    pub fn set_code_with_hash(
        &mut self,
        address: Address,
        code: Bytecode,
        hash: B256,
    ) -> Result<(), EvmInternalsError> {
        self.internals.set_code_with_hash(address, code, hash)
    }

    /// Stores the storage value in Journal state.
    ///
    /// This will load the account and storage value and return an error if database error occurs.
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
