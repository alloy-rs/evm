//! State database abstraction.

use alloy_primitives::Address;
use revm::database::State;

/// A type which has the state of the blockchain.
///
/// This trait encapsulates some of the functionality found in [`State`]
pub trait StateDB: revm::Database {
    /// State clear EIP-161 is enabled in Spurious Dragon hardfork.
    fn set_state_clear_flag(&mut self, has_state_clear: bool);

    /// Iterates over received balances and increment all account balances.
    ///
    /// **Note**: If account is not found inside cache state it will be loaded from database.
    ///
    /// Update will create transitions for all accounts that are updated.
    ///
    /// If using this to implement withdrawals, zero balances must be filtered out before calling
    /// this function.
    fn increment_balances(
        &mut self,
        balances: impl IntoIterator<Item = (Address, u128)>,
    ) -> Result<(), Self::Error>;
}

/// auto_impl unable to reconcile return associated type from supertrait
impl<T: StateDB> StateDB for &mut T {
    fn set_state_clear_flag(&mut self, has_state_clear: bool) {
        StateDB::set_state_clear_flag(*self, has_state_clear);
    }

    fn increment_balances(
        &mut self,
        balances: impl IntoIterator<Item = (Address, u128)>,
    ) -> Result<(), Self::Error> {
        StateDB::increment_balances(*self, balances)
    }
}

impl<DB: revm::Database> StateDB for State<DB> {
    fn set_state_clear_flag(&mut self, has_state_clear: bool) {
        self.cache.set_state_clear_flag(has_state_clear);
    }

    fn increment_balances(
        &mut self,
        balances: impl IntoIterator<Item = (Address, u128)>,
    ) -> Result<(), Self::Error> {
        Self::increment_balances(self, balances)
    }
}
