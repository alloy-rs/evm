//! State database abstraction.

use revm::database::State;

/// A type which has the state of the blockchain.
///
/// This trait encapsulates some of the functionality found in [`State`]
pub trait StateDB: revm::Database {
    /// State clear EIP-161 is enabled in Spurious Dragon hardfork.
    fn set_state_clear_flag(&mut self, has_state_clear: bool);
}

/// auto_impl unable to reconcile return associated type from supertrait
impl<T: StateDB> StateDB for &mut T {
    fn set_state_clear_flag(&mut self, has_state_clear: bool) {
        StateDB::set_state_clear_flag(*self, has_state_clear);
    }
}

impl<DB: revm::Database> StateDB for State<DB> {
    fn set_state_clear_flag(&mut self, has_state_clear: bool) {
        self.cache.set_state_clear_flag(has_state_clear);
    }
}
