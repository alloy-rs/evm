//! Configuration types for EVM environment.

use core::fmt::Debug;

use alloy_primitives::U256;
use revm::{
    context::{BlockEnv, CfgEnv},
    primitives::hardfork::SpecId,
};

/// Container type that holds both the configuration and block environment for EVM execution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EvmEnv<Spec = SpecId, BlockEnv = revm::context::BlockEnv> {
    /// The configuration environment with handler settings
    pub cfg_env: CfgEnv<Spec>,
    /// The block environment containing block-specific data
    pub block_env: BlockEnv,
}

impl<Spec, BlockEnv> EvmEnv<Spec, BlockEnv> {
    /// Create a new `EvmEnv` from its components.
    ///
    /// # Arguments
    ///
    /// * `cfg_env_with_handler_cfg` - The configuration environment with handler settings
    /// * `block` - The block environment containing block-specific data
    pub const fn new(cfg_env: CfgEnv<Spec>, block_env: BlockEnv) -> Self {
        Self { cfg_env, block_env }
    }
}

impl<Spec, BlockEnv: BlockEnvironment> EvmEnv<Spec, BlockEnv> {
    /// Sets an extension on the environment.
    pub fn map_block_env<NewBlockEnv>(
        self,
        f: impl FnOnce(BlockEnv) -> NewBlockEnv,
    ) -> EvmEnv<Spec, NewBlockEnv> {
        let Self { cfg_env, block_env } = self;
        EvmEnv { cfg_env, block_env: f(block_env) }
    }

    /// Returns a reference to the block environment.
    pub const fn block_env(&self) -> &BlockEnv {
        &self.block_env
    }

    /// Returns a reference to the configuration environment.
    pub const fn cfg_env(&self) -> &CfgEnv<Spec> {
        &self.cfg_env
    }

    /// Returns the chain ID of the environment.
    pub const fn chainid(&self) -> u64 {
        self.cfg_env.chain_id
    }

    /// Returns the spec id of the chain
    pub const fn spec_id(&self) -> &Spec {
        &self.cfg_env.spec
    }

    /// Overrides the configured block number
    pub fn with_block_number(mut self, number: U256) -> Self {
        self.block_env.inner_mut().number = number;
        self
    }

    /// Convenience function that overrides the configured block number with the given
    /// `Some(number)`.
    ///
    /// This is intended for block overrides.
    pub fn with_block_number_opt(mut self, number: Option<U256>) -> Self {
        if let Some(number) = number {
            self.block_env.inner_mut().number = number;
        }
        self
    }

    /// Sets the block number if provided.
    pub fn set_block_number_opt(&mut self, number: Option<U256>) -> &mut Self {
        if let Some(number) = number {
            self.block_env.inner_mut().number = number;
        }
        self
    }

    /// Overrides the configured block timestamp.
    pub fn with_timestamp(mut self, timestamp: U256) -> Self {
        self.block_env.inner_mut().timestamp = timestamp;
        self
    }

    /// Convenience function that overrides the configured block timestamp with the given
    /// `Some(timestamp)`.
    ///
    /// This is intended for block overrides.
    pub fn with_timestamp_opt(mut self, timestamp: Option<U256>) -> Self {
        if let Some(timestamp) = timestamp {
            self.block_env.inner_mut().timestamp = timestamp;
        }
        self
    }

    /// Sets the block timestamp if provided.
    pub fn set_timestamp_opt(&mut self, timestamp: Option<U256>) -> &mut Self {
        if let Some(timestamp) = timestamp {
            self.block_env.inner_mut().timestamp = timestamp;
        }
        self
    }

    /// Overrides the configured block base fee.
    pub fn with_base_fee(mut self, base_fee: u64) -> Self {
        self.block_env.inner_mut().basefee = base_fee;
        self
    }

    /// Convenience function that overrides the configured block base fee with the given
    /// `Some(base_fee)`.
    ///
    /// This is intended for block overrides.
    pub fn with_base_fee_opt(mut self, base_fee: Option<u64>) -> Self {
        if let Some(base_fee) = base_fee {
            self.block_env.inner_mut().basefee = base_fee;
        }
        self
    }

    /// Sets the block base fee if provided.
    pub fn set_base_fee_opt(&mut self, base_fee: Option<u64>) -> &mut Self {
        if let Some(base_fee) = base_fee {
            self.block_env.inner_mut().basefee = base_fee;
        }
        self
    }
}

impl<Spec, BlockEnv> From<(CfgEnv<Spec>, BlockEnv)> for EvmEnv<Spec, BlockEnv> {
    fn from((cfg_env, block_env): (CfgEnv<Spec>, BlockEnv)) -> Self {
        Self { cfg_env, block_env }
    }
}

/// Trait for types that can be used as a block environment.
///
/// Assumes that the type wraps an inner [`revm::context::BlockEnv`].
pub trait BlockEnvironment: revm::context::Block + Clone + Debug + Send + Sync + 'static {
    /// Returns a mutable reference to the inner [`revm::context::BlockEnv`].
    fn inner_mut(&mut self) -> &mut revm::context::BlockEnv;
}

impl BlockEnvironment for BlockEnv {
    fn inner_mut(&mut self) -> &mut revm::context::BlockEnv {
        self
    }
}
