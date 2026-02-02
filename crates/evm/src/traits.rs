//! EVM traits.

use crate::Database;
use alloc::boxed::Box;
use alloy_primitives::{Address, Log, B256, U256};
use core::{error::Error, fmt, fmt::Debug};
use revm::{
    context::{Block, DBErrorMarker, JournalTr},
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
    fn load_account(
        &mut self,
        address: Address,
    ) -> Result<StateLoad<&mut Account>, EvmInternalsError>;

    fn load_account_code(
        &mut self,
        address: Address,
    ) -> Result<StateLoad<&mut Account>, EvmInternalsError>;

    fn sload(
        &mut self,
        address: Address,
        key: StorageKey,
    ) -> Result<StateLoad<StorageValue>, EvmInternalsError>;

    fn touch_account(&mut self, address: Address);

    fn set_code(&mut self, address: Address, code: Bytecode);

    fn sstore(
        &mut self,
        address: Address,
        key: StorageKey,
        value: StorageValue,
    ) -> Result<StateLoad<SStoreResult>, EvmInternalsError>;

    fn log(&mut self, log: Log);
}

/// Helper internal struct for implementing [`EvmInternals`].
#[derive(Debug)]
pub(crate) struct EvmInternalsImpl<'a, T>(&'a mut T);

impl<'a, T> EvmInternalsImpl<'a, T> {
    pub(crate) fn new(inner: &'a mut T) -> Self {
        Self(inner)
    }
}

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
    fn load_account(
        &mut self,
        address: Address,
    ) -> Result<StateLoad<&mut Account>, EvmInternalsError> {
        self.0.load_account(address).map_err(EvmInternalsError::database)
    }

    fn load_account_code(
        &mut self,
        address: Address,
    ) -> Result<StateLoad<&mut Account>, EvmInternalsError> {
        self.0.load_account_code(address).map_err(EvmInternalsError::database)
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
}

enum EvmInternalsInner<'a> {
    Boxed(Box<dyn EvmInternalsTr + 'a>),
    Ref(&'a mut dyn EvmInternalsTr),
}

/// Helper type exposing hooks into EVM and access to evm internal settings.
pub struct EvmInternals<'a> {
    internals: EvmInternalsInner<'a>,
    block_env: &'a (dyn Block + 'a),
}

impl<'a> EvmInternals<'a> {
    /// Creates a new [`EvmInternals`] instance.
    pub fn new<T>(journal: &'a mut T, block_env: &'a dyn Block) -> Self
    where
        T: JournalTr<Database: Database> + Debug,
    {
        Self { internals: EvmInternalsInner::Boxed(Box::new(EvmInternalsImpl(journal))), block_env }
    }

    /// Creates an `EvmInternals` backed by an existing `EvmInternalsImpl` to avoid a heap
    /// allocation on hot paths (e.g. precompile execution).
    pub(crate) fn from_impl<'b, T>(
        internals: &'a mut EvmInternalsImpl<'b, T>,
        block_env: &'a dyn Block,
    ) -> Self
    where
        T: JournalTr<Database: Database> + Debug,
    {
        Self { internals: EvmInternalsInner::Ref(internals), block_env }
    }

    fn internals(&self) -> &dyn EvmInternalsTr {
        match &self.internals {
            EvmInternalsInner::Boxed(internals) => internals.as_ref(),
            EvmInternalsInner::Ref(internals) => &**internals,
        }
    }

    #[inline]
    fn internals_mut(&mut self) -> &mut dyn EvmInternalsTr {
        match &mut self.internals {
            EvmInternalsInner::Boxed(internals) => internals.as_mut(),
            EvmInternalsInner::Ref(internals) => &mut **internals,
        }
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
        self.internals_mut()
    }

    /// Loads an account.
    pub fn load_account(
        &mut self,
        address: Address,
    ) -> Result<StateLoad<&mut Account>, EvmInternalsError> {
        self.internals_mut().load_account(address)
    }

    /// Loads code of an account.
    pub fn load_account_code(
        &mut self,
        address: Address,
    ) -> Result<StateLoad<&mut Account>, EvmInternalsError> {
        self.internals_mut().load_account_code(address)
    }

    /// Loads a storage slot.
    pub fn sload(
        &mut self,
        address: Address,
        key: StorageKey,
    ) -> Result<StateLoad<StorageValue>, EvmInternalsError> {
        self.internals_mut().sload(address, key)
    }

    /// Touches the account.
    pub fn touch_account(&mut self, address: Address) {
        self.internals_mut().touch_account(address);
    }

    /// Sets bytecode to the account.
    pub fn set_code(&mut self, address: Address, code: Bytecode) {
        self.internals_mut().set_code(address, code);
    }

    /// Stores the storage value in Journal state.
    pub fn sstore(
        &mut self,
        address: Address,
        key: StorageKey,
        value: StorageValue,
    ) -> Result<StateLoad<SStoreResult>, EvmInternalsError> {
        self.internals_mut().sstore(address, key, value)
    }

    /// Logs the log in Journal state.
    pub fn log(&mut self, log: Log) {
        self.internals_mut().log(log);
    }
}

impl<'a> fmt::Debug for EvmInternals<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EvmInternals")
            .field("internals", &self.internals())
            .field("block_env", &"{{}}")
            .finish_non_exhaustive()
    }
}
