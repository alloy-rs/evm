//! State database abstraction.
use core::error::Error;

use alloy_primitives::Address;
use revm::database::State;

/// A type which has the state of the blockchain.
///
/// This trait encapsulates some of the functionality found in [`State`]
#[auto_impl::auto_impl(&mut)]
pub trait StateDB {
    /// The database error type.
    type Error: Error;

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

impl<DB: revm::Database> StateDB for State<DB> {
    type Error = DB::Error;

    fn set_state_clear_flag(&mut self, has_state_clear: bool) {
        self.cache.set_state_clear_flag(has_state_clear);
    }

    fn increment_balances(
        &mut self,
        balances: impl IntoIterator<Item = (Address, u128)>,
    ) -> Result<(), Self::Error> {
        self.increment_balances(balances)
    }
}
