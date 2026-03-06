//! State database abstraction.

use crate::Database;
use alloy_eips::eip7928::BlockAccessList;
use revm::{database::State, state::bal::Bal, DatabaseCommit};

/// Alias trait for [`Database`] and [`DatabaseCommit`].
pub trait StateDB: Database + DatabaseCommit {
    /// Bumps the balance index.
    fn bump_bal_index(&mut self);
    /// Takes the built Alloy BAL.
    fn take_built_alloy_bal(&mut self) -> Option<BlockAccessList>;
    /// Sets bal builder with the provided bal.
    fn set_bal(&mut self, bal: Option<Bal>);
}

impl<DB> StateDB for State<DB>
where
    DB: Database + DatabaseCommit,
{
    fn bump_bal_index(&mut self) {
        self.bal_state.bump_bal_index();
    }

    fn take_built_alloy_bal(&mut self) -> Option<BlockAccessList> {
        self.bal_state.take_built_alloy_bal()
    }

    fn set_bal(&mut self, bal: Option<Bal>) {
        self.bal_state.bal_builder = bal;
    }
}
