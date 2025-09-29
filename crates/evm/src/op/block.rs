//! Optimism block execution result types.

use crate::block::BlockExecutionResult;

/// The result of executing a block.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpBlockExecutionResult<T> {
    /// Legacy block execution result.
    pub result: BlockExecutionResult<T>,
    /// Blob gas usage for the block. This is only relevant after the Jovian hardfork.
    pub blob_gas_used: Option<u64>,
}

impl<T> From<BlockExecutionResult<T>> for OpBlockExecutionResult<T> {
    fn from(result: BlockExecutionResult<T>) -> Self {
        Self { result, blob_gas_used: None }
    }
}
