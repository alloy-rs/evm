//! State database abstraction.

use crate::Database;
use revm::{database::State, DatabaseCommit};

/// A type which has the state of the blockchain.
///
/// This trait encapsulates some of the functionality found in [`State`]
#[auto_impl::auto_impl(&mut, Box)]
pub trait StateDB: Database + DatabaseCommit {
    /// State clear EIP-161 is enabled in Spurious Dragon hardfork.
    fn set_state_clear_flag(&mut self, has_state_clear: bool);
}

impl<DB: Database> StateDB for State<DB> {
    fn set_state_clear_flag(&mut self, has_state_clear: bool) {
        self.cache.set_state_clear_flag(has_state_clear);
    }
}
