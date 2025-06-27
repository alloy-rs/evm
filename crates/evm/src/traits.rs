//! EVM traits.

use crate::error::EvmInternalsError;
use alloy_primitives::{Address, B256, U256};
use core::fmt::Debug;

/// Account information for EVM internals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvmAccount {
    /// Account balance.
    pub balance: U256,
    /// Account nonce.
    pub nonce: u64,
    /// Account code hash.
    pub code_hash: Option<B256>,
    /// Account code.
    pub code: Option<Vec<u8>>,
}

/// Object-safe trait for accessing and modifying EVM internals, particularly the journal.
///
/// This trait provides an abstraction over journal operations without exposing
/// associated types, making it object-safe and suitable for dynamic dispatch.
pub trait EvmInternals: Debug + Send + Sync {
    /// Get an account from the journaled state.
    fn get_account(&mut self, address: Address) -> Result<Option<EvmAccount>, EvmInternalsError>;

    /// Set or update an account in the journaled state.
    fn set_account(
        &mut self,
        address: Address,
        account: EvmAccount,
    ) -> Result<(), EvmInternalsError>;

    /// Get the storage value for an account.
    fn get_storage(
        &mut self,
        address: Address,
        slot: U256,
    ) -> Result<Option<U256>, EvmInternalsError>;

    /// Set the storage value for an account.
    fn set_storage(
        &mut self,
        address: Address,
        slot: U256,
        value: U256,
    ) -> Result<(), EvmInternalsError>;

    /// Get the code for an account.
    fn get_code(&mut self, address: Address) -> Result<Option<Vec<u8>>, EvmInternalsError>;

    /// Set the code for an account.
    fn set_code(&mut self, address: Address, code: Vec<u8>) -> Result<(), EvmInternalsError>;

    /// Get the code hash for an account.
    fn get_code_hash(&mut self, address: Address) -> Result<Option<B256>, EvmInternalsError>;

    /// Check if an account exists.
    fn account_exists(&mut self, address: Address) -> Result<bool, EvmInternalsError>;

    /// Mark an account as touched.
    fn touch_account(&mut self, address: Address) -> Result<(), EvmInternalsError>;

    /// Create a new account.
    fn create_account(
        &mut self,
        address: Address,
        balance: U256,
        nonce: u64,
    ) -> Result<(), EvmInternalsError>;

    /// Delete an account.
    fn delete_account(&mut self, address: Address) -> Result<(), EvmInternalsError>;

    /// Check if an account was created in the current transaction.
    fn is_account_created(&self, address: Address) -> Result<bool, EvmInternalsError>;

    /// Check if an account was destroyed in the current transaction.
    fn is_account_destroyed(&self, address: Address) -> Result<bool, EvmInternalsError>;

    /// Transfer balance between accounts.
    fn transfer(
        &mut self,
        from: Address,
        to: Address,
        amount: U256,
    ) -> Result<(), EvmInternalsError>;

    /// Add balance to an account.
    fn add_balance(&mut self, address: Address, amount: U256) -> Result<(), EvmInternalsError>;

    /// Subtract balance from an account.
    fn sub_balance(&mut self, address: Address, amount: U256) -> Result<(), EvmInternalsError>;

    /// Get the balance of an account.
    fn get_balance(&mut self, address: Address) -> Result<U256, EvmInternalsError>;

    /// Get the nonce of an account.
    fn get_nonce(&mut self, address: Address) -> Result<u64, EvmInternalsError>;

    /// Set the nonce of an account.
    fn set_nonce(&mut self, address: Address, nonce: u64) -> Result<(), EvmInternalsError>;

    /// Increment the nonce of an account.
    fn increment_nonce(&mut self, address: Address) -> Result<(), EvmInternalsError>;

    /// Create a checkpoint in the journal.
    fn checkpoint(&mut self) -> Result<(), EvmInternalsError>;

    /// Revert to the last checkpoint.
    fn revert_to_checkpoint(&mut self) -> Result<(), EvmInternalsError>;

    /// Commit the current checkpoint.
    fn commit_checkpoint(&mut self) -> Result<(), EvmInternalsError>;

    /// Get the current depth of checkpoints.
    fn checkpoint_depth(&self) -> Result<usize, EvmInternalsError>;

    /// Clear all journal entries.
    fn clear_journal(&mut self) -> Result<(), EvmInternalsError>;

    /// Get all touched addresses.
    fn get_touched_addresses(&self) -> Result<Vec<Address>, EvmInternalsError>;

    /// Get all created addresses.
    fn get_created_addresses(&self) -> Result<Vec<Address>, EvmInternalsError>;

    /// Get all destroyed addresses.
    fn get_destroyed_addresses(&self) -> Result<Vec<Address>, EvmInternalsError>;

    /// Get all storage slots that have been modified for an account.
    /// Returns a vector of (slot, value) pairs.
    fn get_modified_storage(
        &self,
        address: Address,
    ) -> Result<Vec<(U256, U256)>, EvmInternalsError>;
}

