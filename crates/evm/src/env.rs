//! Configuration types for EVM environment.

use core::fmt::Debug;

use alloy_primitives::U256;
use revm::{
    context::{BlockEnv, CfgEnv},
    primitives::hardfork::SpecId,
};

/// Container type that holds both the configuration and block environment for EVM execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvmEnv<Spec = SpecId, BlockEnv = revm::context::BlockEnv> {
    /// The configuration environment with handler settings
    pub cfg_env: CfgEnv<Spec>,
    /// The block environment containing block-specific data
    pub block_env: BlockEnv,
}

impl<Spec: Default + Into<SpecId> + Clone, B: Default> Default for EvmEnv<Spec, B> {
    fn default() -> Self {
        Self { cfg_env: CfgEnv::new_with_spec(Spec::default()), block_env: B::default() }
    }
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

    /// Configures the EVM execution limits.
    ///
    /// Sets `limit_contract_code_size`, `limit_contract_initcode_size`,
    /// and `tx_gas_limit_cap` from the provided [`EvmLimitParams`].
    pub const fn with_limits(mut self, limits: EvmLimitParams) -> Self {
        self.cfg_env.limit_contract_code_size = Some(limits.max_code_size);
        self.cfg_env.limit_contract_initcode_size = Some(limits.max_initcode_size);
        self.cfg_env.tx_gas_limit_cap = limits.tx_gas_limit_cap;
        self
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

/// Parameters for EVM execution limits.
///
/// These parameters control configurable limits in the EVM that can be
/// overridden from their spec defaults (EIP-170, EIP-3860, EIP-7825).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvmLimitParams {
    /// Maximum bytecode size for deployed contracts.
    /// EIP-170 default: 24576 bytes (24KB)
    pub max_code_size: usize,
    /// Maximum initcode size for CREATE transactions.
    /// EIP-3860 default: 49152 bytes (48KB, 2x `max_code_size`)
    pub max_initcode_size: usize,
    /// Transaction gas limit cap.
    /// - `None` = use spec default (respects fork-aware defaults like EIP-7825)
    /// - `Some(cap)` = transactions with `gas_limit > cap` are rejected
    pub tx_gas_limit_cap: Option<u64>,
}

impl Default for EvmLimitParams {
    fn default() -> Self {
        Self {
            max_code_size: revm::primitives::eip170::MAX_CODE_SIZE,
            max_initcode_size: revm::primitives::eip3860::MAX_INITCODE_SIZE,
            tx_gas_limit_cap: None,
        }
    }
}

impl EvmLimitParams {
    /// Returns the Osaka EVM limit params.
    pub const fn osaka() -> Self {
        Self {
            max_code_size: revm::primitives::eip170::MAX_CODE_SIZE,
            max_initcode_size: revm::primitives::eip3860::MAX_INITCODE_SIZE,
            tx_gas_limit_cap: Some(revm::primitives::eip7825::TX_GAS_LIMIT_CAP),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use revm::context::Cfg;

    #[test]
    fn test_evm_env_with_limits() {
        let limits = EvmLimitParams {
            max_code_size: 1234,
            max_initcode_size: 5678,
            tx_gas_limit_cap: Some(999_999),
        };

        let evm_env: EvmEnv<SpecId> = EvmEnv::default().with_limits(limits);

        assert_eq!(evm_env.cfg_env.max_code_size(), 1234);
        assert_eq!(evm_env.cfg_env.max_initcode_size(), 5678);
        assert_eq!(evm_env.cfg_env.tx_gas_limit_cap(), 999_999);
    }

    #[test]
    fn test_evm_env_with_default_limits() {
        // default() has tx_gas_limit_cap: None, which respects the spec default.
        let limits = EvmLimitParams::default();
        let evm_env: EvmEnv<SpecId> = EvmEnv::default().with_limits(limits);

        assert_eq!(evm_env.cfg_env.max_code_size(), revm::primitives::eip170::MAX_CODE_SIZE);
        assert_eq!(
            evm_env.cfg_env.max_initcode_size(),
            revm::primitives::eip3860::MAX_INITCODE_SIZE
        );
        // None respects the spec's default (u64::MAX for pre-Osaka specs)
        assert_eq!(evm_env.cfg_env.tx_gas_limit_cap(), u64::MAX);
    }

    #[test]
    fn test_evm_env_with_osaka_limits() {
        // osaka() has tx_gas_limit_cap set to EIP-7825's cap.
        use revm::context::{BlockEnv, CfgEnv};

        let limits = EvmLimitParams::osaka();
        let cfg_env = CfgEnv::new_with_spec(SpecId::OSAKA);
        let evm_env = EvmEnv::new(cfg_env, BlockEnv::default()).with_limits(limits);

        assert_eq!(evm_env.cfg_env.tx_gas_limit_cap(), revm::primitives::eip7825::TX_GAS_LIMIT_CAP);
    }

    #[test]
    fn test_default_respects_osaka_fork_default() {
        // With Osaka spec and default() limits (tx_gas_limit_cap: None),
        // the spec's fork-aware default is respected.
        use revm::context::{BlockEnv, CfgEnv};

        let limits = EvmLimitParams::default(); // tx_gas_limit_cap: None
        let cfg_env = CfgEnv::new_with_spec(SpecId::OSAKA);
        let evm_env = EvmEnv::new(cfg_env, BlockEnv::default()).with_limits(limits);

        // None respects Osaka's default (EIP-7825 cap)
        assert_eq!(evm_env.cfg_env.tx_gas_limit_cap(), revm::primitives::eip7825::TX_GAS_LIMIT_CAP);
    }
}
