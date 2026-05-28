//! State database abstraction.

use crate::Database;
use revm::DatabaseCommit;

/// Alias trait for [`Database`] and [`DatabaseCommit`].
pub trait StateDB: Database + DatabaseCommit {}

impl<T> StateDB for T where T: Database + DatabaseCommit {}
