//! EVM traits.

use crate::Database;
use alloc::boxed::Box;
use alloy_primitives::{Address, Bytes, Log, TxKind, B256, U256};
use core::{error::Error, fmt, fmt::Debug};
use revm::{
    context::{result::InvalidTransaction, Block, DBErrorMarker, JournalTr, Transaction},
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

/// dyn-compatible trait for accessing transaction fields.
pub trait TransactionTr {
    /// Returns the transaction type.
    ///
    /// Depending on this field other functions should be called.
    fn tx_type(&self) -> u8;

    /// Caller aka Author aka transaction signer.
    ///
    /// Note : Common field for all transactions.
    fn caller(&self) -> Address;

    /// The maximum amount of gas the transaction can use.
    ///
    /// Note : Common field for all transactions.
    fn gas_limit(&self) -> u64;

    /// The value sent to the receiver of [`TxKind::Call`].
    ///
    /// Note : Common field for all transactions.
    fn value(&self) -> U256;

    /// Returns the input data of the transaction.
    ///
    /// Note : Common field for all transactions.
    fn input(&self) -> &Bytes;

    /// The nonce of the transaction.
    ///
    /// Note : Common field for all transactions.
    fn nonce(&self) -> u64;

    /// Transaction kind. It can be Call or Create.
    ///
    /// Kind is applicable for: Legacy, EIP-2930, EIP-1559
    /// And is Call for EIP-4844 and EIP-7702 transactions.
    fn kind(&self) -> TxKind;

    /// Chain Id is optional for legacy transactions.
    ///
    /// As it was introduced in EIP-155.
    fn chain_id(&self) -> Option<u64>;

    /// Gas price for the transaction.
    /// It is only applicable for Legacy and EIP-2930 transactions.
    /// For Eip1559 it is max_fee_per_gas.
    fn gas_price(&self) -> u128;

    /// Returns vector of fixed size hash(32 bytes)
    ///
    /// Note : EIP-4844 transaction field.
    fn blob_versioned_hashes(&self) -> &[B256];

    /// Max fee per data gas
    ///
    /// Note : EIP-4844 transaction field.
    fn max_fee_per_blob_gas(&self) -> u128;

    /// Total gas for all blobs. Max number of blocks is already checked
    /// so we dont need to check for overflow.
    fn total_blob_gas(&self) -> u64;

    /// Calculates the maximum [EIP-4844] `data_fee` of the transaction.
    ///
    /// This is used for ensuring that the user has at least enough funds to pay the
    /// `max_fee_per_blob_gas * total_blob_gas`, on top of regular gas costs.
    ///
    /// See EIP-4844:
    /// <https://github.com/ethereum/EIPs/blob/master/EIPS/eip-4844.md#execution-layer-validation>
    fn calc_max_data_fee(&self) -> U256;

    /// Returns length of the authorization list.
    ///
    /// # Note
    ///
    /// Transaction is considered invalid if list is empty.
    fn authorization_list_len(&self) -> usize;

    /// Returns maximum fee that can be paid for the transaction.
    fn max_fee_per_gas(&self) -> u128;

    /// Maximum priority fee per gas.
    fn max_priority_fee_per_gas(&self) -> Option<u128>;

    /// Returns effective gas price is gas price field for Legacy and Eip2930 transaction.
    ///
    /// While for transactions after Eip1559 it is minimum of max_fee and `base + max_priority_fee`.
    fn effective_gas_price(&self, base_fee: u128) -> u128;

    /// Returns the maximum balance that can be spent by the transaction.
    ///
    /// Return U256 or error if all values overflow U256 number.
    fn max_balance_spending(&self) -> Result<U256, InvalidTransaction>;

    /// Returns the effective balance that is going to be spent that depends on base_fee
    /// Multiplication for gas are done in u128 type (saturated) and value is added as U256 type.
    ///
    /// # Reason
    ///
    /// This is done for performance reasons and it is known to be safe as there is no more that
    /// u128::MAX value of eth in existence.
    ///
    /// This is always strictly less than [`Self::max_balance_spending`].
    ///
    /// Return U256 or error if all values overflow U256 number.
    fn effective_balance_spending(
        &self,
        base_fee: u128,
        blob_price: u128,
    ) -> Result<U256, InvalidTransaction>;
}
/// Helper internal struct for implementing [`TransactionTr`].
struct TransactionImpl<'a, T>(pub &'a T);

impl<T> TransactionTr for TransactionImpl<'_, T>
where
    T: Transaction,
{
    fn caller(&self) -> Address {
        self.0.caller()
    }

    fn value(&self) -> U256 {
        self.0.value()
    }
    fn tx_type(&self) -> u8 {
        self.0.tx_type()
    }
    fn gas_limit(&self) -> u64 {
        self.0.gas_limit()
    }
    fn input(&self) -> &Bytes {
        self.0.input()
    }
    fn nonce(&self) -> u64 {
        self.0.nonce()
    }
    fn kind(&self) -> TxKind {
        self.0.kind()
    }
    fn chain_id(&self) -> Option<u64> {
        self.0.chain_id()
    }
    fn gas_price(&self) -> u128 {
        self.0.gas_price()
    }
    fn blob_versioned_hashes(&self) -> &[B256] {
        self.0.blob_versioned_hashes()
    }
    fn max_fee_per_blob_gas(&self) -> u128 {
        self.0.max_fee_per_blob_gas()
    }
    fn total_blob_gas(&self) -> u64 {
        self.0.total_blob_gas()
    }
    fn calc_max_data_fee(&self) -> U256 {
        self.0.calc_max_data_fee()
    }
    fn authorization_list_len(&self) -> usize {
        self.0.authorization_list_len()
    }

    fn max_fee_per_gas(&self) -> u128 {
        self.0.max_fee_per_gas()
    }

    fn max_priority_fee_per_gas(&self) -> Option<u128> {
        self.0.max_priority_fee_per_gas()
    }

    fn effective_gas_price(&self, base_fee: u128) -> u128 {
        self.0.effective_gas_price(base_fee)
    }

    fn max_balance_spending(&self) -> Result<U256, InvalidTransaction> {
        self.0.max_balance_spending()
    }

    fn effective_balance_spending(
        &self,
        base_fee: u128,
        blob_price: u128,
    ) -> Result<U256, InvalidTransaction> {
        self.0.effective_balance_spending(base_fee, blob_price)
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

/// Helper type exposing hooks into EVM and access to evm internal settings.
pub struct EvmInternals<'a> {
    internals: Box<dyn EvmInternalsTr + 'a>,
    block_env: &'a (dyn Block + 'a),
    tx_env: Box<dyn TransactionTr + 'a>,
}

impl<'a> EvmInternals<'a> {
    /// Creates a new [`EvmInternals`] instance.
    pub fn new<T, TX>(journal: &'a mut T, block_env: &'a dyn Block, tx_env: &'a TX) -> Self
    where
        T: JournalTr<Database: Database> + Debug,
        TX: Transaction,
    {
        Self {
            internals: Box::new(EvmInternalsImpl(journal)),
            block_env,
            tx_env: Box::new(TransactionImpl(tx_env)),
        }
    }

    /// Returns the  evm's block information.
    pub const fn block_env(&self) -> impl Block + 'a {
        self.block_env
    }

    /// Returns the evm's transaction information.
    pub fn tx_env(&self) -> &dyn TransactionTr {
        &*self.tx_env
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
    ) -> Result<StateLoad<&mut Account>, EvmInternalsError> {
        self.internals.load_account(address)
    }

    /// Loads code of an account.
    pub fn load_account_code(
        &mut self,
        address: Address,
    ) -> Result<StateLoad<&mut Account>, EvmInternalsError> {
        self.internals.load_account_code(address)
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
}

impl<'a> fmt::Debug for EvmInternals<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EvmInternals")
            .field("internals", &self.internals)
            .field("block_env", &"{{}}")
            .finish_non_exhaustive()
    }
}
