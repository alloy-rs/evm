//! Configuration types for EVM environment.

use alloy_primitives::U256;
use revm::{
    context::{BlockEnv, CfgEnv},
    primitives::hardfork::SpecId,
};

/// Container type that holds both the configuration and block environment for EVM execution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EvmEnv<Spec = SpecId> {
    /// The configuration environment with handler settings
    pub cfg_env: CfgEnv<Spec>,
    /// The block environment containing block-specific data
    pub block_env: BlockEnv,
}

impl<Spec> EvmEnv<Spec> {
    /// Create a new `EvmEnv` from its components.
    ///
    /// # Arguments
    ///
    /// * `cfg_env_with_handler_cfg` - The configuration environment with handler settings
    /// * `block` - The block environment containing block-specific data
    pub const fn new(cfg_env: CfgEnv<Spec>, block_env: BlockEnv) -> Self {
        Self { cfg_env, block_env }
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
    pub const fn with_block_number(mut self, number: U256) -> Self {
        self.block_env.number = number;
        self
    }

    /// Convenience function that overrides the configured block number with the given
    /// `Some(number)`.
    ///
    /// This is intended for block overrides.
    pub const fn with_block_number_opt(mut self, number: Option<U256>) -> Self {
        if let Some(number) = number {
            self.block_env.number = number;
        }
        self
    }

    /// Sets the block number if provided.
    pub const fn set_block_number_opt(&mut self, number: Option<U256>) -> &mut Self {
        if let Some(number) = number {
            self.block_env.number = number;
        }
        self
    }

    /// Overrides the configured block timestamp.
    pub const fn with_timestamp(mut self, timestamp: U256) -> Self {
        self.block_env.timestamp = timestamp;
        self
    }

    /// Convenience function that overrides the configured block timestamp with the given
    /// `Some(timestamp)`.
    ///
    /// This is intended for block overrides.
    pub const fn with_timestamp_opt(mut self, timestamp: Option<U256>) -> Self {
        if let Some(timestamp) = timestamp {
            self.block_env.timestamp = timestamp;
        }
        self
    }

    /// Sets the block timestamp if provided.
    pub const fn set_timestamp_opt(&mut self, timestamp: Option<U256>) -> &mut Self {
        if let Some(timestamp) = timestamp {
            self.block_env.timestamp = timestamp;
        }
        self
    }

    /// Overrides the configured block base fee.
    pub const fn with_base_fee(mut self, base_fee: u64) -> Self {
        self.block_env.basefee = base_fee;
        self
    }

    /// Convenience function that overrides the configured block base fee with the given
    /// `Some(base_fee)`.
    ///
    /// This is intended for block overrides.
    pub const fn with_base_fee_opt(mut self, base_fee: Option<u64>) -> Self {
        if let Some(base_fee) = base_fee {
            self.block_env.basefee = base_fee;
        }
        self
    }

    /// Sets the block base fee if provided.
    pub const fn set_base_fee_opt(&mut self, base_fee: Option<u64>) -> &mut Self {
        if let Some(base_fee) = base_fee {
            self.block_env.basefee = base_fee;
        }
        self
    }
}

impl<Spec> From<(CfgEnv<Spec>, BlockEnv)> for EvmEnv<Spec> {
    fn from((cfg_env, block_env): (CfgEnv<Spec>, BlockEnv)) -> Self {
        Self { cfg_env, block_env }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eth::spec::EthSpec;
    use alloy_consensus::Header;
    use alloy_hardforks::ethereum::MAINNET_PARIS_BLOCK;
    use alloy_primitives::B256;

    #[test_case::test_case(
        Header::default(),
        EvmEnv {
            cfg_env: CfgEnv::new_with_spec(SpecId::FRONTIER).with_chain_id(2),
            block_env: BlockEnv {
                timestamp: U256::ZERO,
                gas_limit: 0,
                prevrandao: None,
                blob_excess_gas_and_price: None,
                ..BlockEnv::default()
            },
        };
        "Frontier"
    )]
    #[test_case::test_case(
        Header {
            number: MAINNET_PARIS_BLOCK,
            mix_hash: B256::with_last_byte(2),
            ..Header::default()
        },
        EvmEnv {
            cfg_env: CfgEnv::new_with_spec(SpecId::MERGE).with_chain_id(2),
            block_env: BlockEnv {
                number: U256::from(MAINNET_PARIS_BLOCK),
                timestamp: U256::ZERO,
                gas_limit: 0,
                prevrandao: Some(B256::with_last_byte(2)),
                blob_excess_gas_and_price: None,
                ..BlockEnv::default()
            },
        };
        "Paris"
    )]
    fn test_evm_env_is_consistent_with_given_block(
        header: Header,
        expected_evm_env: EvmEnv<SpecId>,
    ) {
        let chain_id = 2;
        let spec = EthSpec::mainnet();
        let blob_params = None;
        let actual_evm_env = EvmEnv::for_eth_block(header, spec, chain_id, blob_params);

        assert_eq!(actual_evm_env, expected_evm_env);
    }
}
