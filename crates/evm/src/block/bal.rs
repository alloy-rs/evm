//! Helper types for [EIP-7928](https://eips.ethereum.org/EIPS/eip-7928#asynchronous-validation) aware execution.

use crate::block::BlockExecutionError;

/// Required context for executing a BAL transaction.
#[derive(Debug)]
pub struct BalTxContext {
    /// The mandatory input for this transaction.
    ///
    /// This is derived from the state changes in previous transactions in the block.
    pub input: TxInput,
    /// The expected output set for this specific transaction
    pub output: BalTxExpectedOutput,
}

/// The exection output of a BAL aware transaction
#[derive(Debug)]
pub struct BalTxOutput {}

/// The transaction specific input set.
///
/// This is similar to state overrides that must be applied before executing the transaction.
/// For example any account/storage changes from previous transactions in the block.
#[derive(Debug)]
pub struct TxInput {}

/// The expected changes performed by the transaction.
///
/// This is effectively the write portion of the [`EvmState`] in the execution result and must be
/// complete match.
#[derive(Debug)]
pub struct BalTxExpectedOutput {}

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
