//! Optimism EVM implementation.

mod block;
mod env;
mod spec_id;

pub use block::OpBlockExecutionResult;
pub use spec_id::{spec, spec_by_timestamp_after_bedrock};
