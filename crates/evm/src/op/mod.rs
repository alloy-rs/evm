//! Optimism EVM implementation.

mod env;
mod spec_id;
mod tx;

pub use spec_id::{spec, spec_by_timestamp_after_bedrock};
