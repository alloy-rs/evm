//! Helper types for [EIP-7928](https://eips.ethereum.org/EIPS/eip-7928#asynchronous-validation) aware execution.

use alloy_eip7928::AccountChanges;
use alloy_primitives::{Address, B256};
use alloy_primitives::map::{HashMap, HashSet};
use revm::bytecode::Bytecode;
use revm::Database;
use revm::primitives::{StorageKey, StorageValue};
use revm::state::AccountInfo;
use crate::block::BlockExecutionError;

/// A Block level access list aware database.
pub trait BalDb {

    /// Sets the index of the transaction that's being executed.
    fn set_transaction_index(&mut self, index: usize);

}

/// A block level accesslist aware database/state implementation that is diff aware.
///
/// This type is intended to be used for block level execution.
#[derive(Debug)]
pub struct BalState<DB> {
    /// The underlying database used to fetch reads from.
    inner: DB,
    /// The block level
    bal : IndexedBal,
    /// Tracks the index of the transaction that is currently being executed.
    current_tx_index: usize,
}

impl<DB> BalState<DB> {

    /// Marks the execution as completed
    fn finish(&mut self) {
        // TODO: do we need this?
    }

}

impl<DB> BalDb for BalState<DB> {
    fn set_transaction_index(&mut self, index: usize) {
        self.current_tx_index = index;
    }
}

impl<DB> Database for BalState<DB>
where DB: Database
{
    type Error = <DB as Database>::Error;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        todo!()
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        todo!()
    }

    fn storage(&mut self, address: Address, index: StorageKey) -> Result<StorageValue, Self::Error> {
        todo!()
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        todo!()
    }
}


/// A transaction indexed representation of EIP-7928 accesslist.
/// TODO: Arc this
#[derive(Debug, Clone)]
pub struct IndexedBal {
    /// All accounts grouped by their address.
    accounts: HashMap<Address, AccountChanges>
}

impl IndexedBal {

    /// Creates a new instance from the given list of account changes
    pub fn new(bal: Vec<AccountChanges>) -> Self {
        Self {
            accounts: bal.into_iter().map(|change|(change.address, change)).collect(),
        }
    }

    /// Returns the account
    fn account_at(&self, address: &Address, idx: usize) -> Option<AccountInfo> {
        None
    }


}


// /// Required context for executing a BAL transaction.
// #[derive(Debug)]
// pub struct BalTxContext {
//     /// The mandatory input for this transaction.
//     ///
//     /// This is derived from the state changes in previous transactions in the block.
//     pub input: TxInput,
//     /// The expected output set for this specific transaction
//     pub output: BalTxExpectedOutput,
// }
//
// /// The exection output of a BAL aware transaction
// #[derive(Debug)]
// pub struct BalTxOutput {}
//
// /// The transaction specific input set.
// ///
// /// This is similar to state overrides that must be applied before executing the transaction.
// /// For example any account/storage changes from previous transactions in the block.
// #[derive(Debug)]
// pub struct TxInput {}

// /// The expected changes performed by the transaction.
// ///
// /// This is effectively the write portion of the [`EvmState`] in the execution result and must be
// /// complete match.
// #[derive(Debug)]
// pub struct BalTxExpectedOutput {}

/// A BAL aware transaction failed to execute
#[derive(Debug)]
pub enum BalExecutionError {
    /// Output of the transaction invalidated the BAL input.
    BalDiff(BalDiffError),
    /// Executing the transaction failed.
    Execution(BlockExecutionError),
}

/// A BAL aware transaction execution resulted in an invalid BAL.
#[derive(Debug)]
pub enum BalDiffError {}
