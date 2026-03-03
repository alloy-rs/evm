//! State database abstraction.

use crate::Database;
use revm::{database::State, DatabaseCommit};

/// A type which has the state of the blockchain.
///
/// This trait encapsulates some of the functionality found in [`State`]
#[auto_impl::auto_impl(&mut, Box)]
pub trait StateDB: Database + DatabaseCommit {}

impl<DB: Database> StateDB for State<DB> {}
