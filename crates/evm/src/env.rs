//! Configuration types for EVM environment.

use alloy_consensus::BlockHeader;
use alloy_eips::eip7840::BlobParams;
use alloy_hardforks::EthereumHardforks;
use alloy_primitives::{ChainId, U256};
use revm::{
    context::{BlockEnv, CfgEnv},
    context_interface::block::BlobExcessGasAndPrice,
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

impl EvmEnv<SpecId> {
    /// Create a new `EvmEnv` from a block `header`, `chain_id`, chain `spec` and optional
    /// `blob_params`.
    ///
    /// # Arguments
    ///
    /// * `header` - The block to make the env out of.
    /// * `chain_spec` - The chain hardfork description, must implement [`EthereumHardforks`].
    /// * `chain_id` - The chain identifier.
    /// * `blob_params` - Optional parameters that sets limits on gas and count for blobs.
    pub fn for_eth_block(
        header: impl BlockHeader,
        chain_spec: impl EthereumHardforks,
        chain_id: ChainId,
        blob_params: Option<BlobParams>,
    ) -> Self {
        Self::for_block(header, chain_spec, chain_id, blob_params, crate::spec)
    }
}

impl<Spec> EvmEnv<Spec> {
    /// Create a new `EvmEnv` from a block `header`, `chain_id`, chain `spec` and optional
    /// `blob_params`.
    ///
    /// # Arguments
    ///
    /// * `header` - The block to make the env out of.
    /// * `chain_spec` - The chain hardfork description, must implement [`EthereumHardforks`].
    /// * `chain_id` - The chain identifier.
    /// * `blob_params` - Optional parameters that sets limits on gas and count for blobs.
    /// * `spec_factory` - A function that derives `Spec` from `chain_spec` and `header`.
    pub fn for_block<H, C>(
        header: H,
        chain_spec: C,
        chain_id: ChainId,
        blob_params: Option<BlobParams>,
        spec_factory: impl FnOnce(&C, &H) -> Spec,
    ) -> Self
    where
        C: EthereumHardforks,
        H: BlockHeader,
    {
        /// Maximum transaction gas limit as defined by [EIP-7825](https://eips.ethereum.org/EIPS/eip-7825) activated in `Osaka` hardfork.
        const MAX_TX_GAS_LIMIT_OSAKA: u64 = 2u64.pow(24);

        let spec = spec_factory(&chain_spec, &header);
        let mut cfg_env = CfgEnv::new_with_spec(spec).with_chain_id(chain_id);

        if let Some(blob_params) = &blob_params {
            cfg_env.set_max_blobs_per_tx(blob_params.max_blobs_per_tx);
        }

        if chain_spec.is_osaka_active_at_timestamp(header.timestamp()) {
            cfg_env.tx_gas_limit_cap = Some(MAX_TX_GAS_LIMIT_OSAKA);
        }

        // derive the EIP-4844 blob fees from the header's `excess_blob_gas` and the current
        // blobparams
        let blob_excess_gas_and_price =
            header.excess_blob_gas().zip(blob_params).map(|(excess_blob_gas, params)| {
                let blob_gasprice = params.calc_blob_fee(excess_blob_gas);
                BlobExcessGasAndPrice { excess_blob_gas, blob_gasprice }
            });

        let is_merge_active = chain_spec.is_paris_active_at_block(header.number());

        let block_env = BlockEnv {
            number: U256::from(header.number()),
            beneficiary: header.beneficiary(),
            timestamp: U256::from(header.timestamp()),
            difficulty: if is_merge_active { U256::ZERO } else { header.difficulty() },
            prevrandao: if is_merge_active { header.mix_hash() } else { None },
            gas_limit: header.gas_limit(),
            basefee: header.base_fee_per_gas().unwrap_or_default(),
            blob_excess_gas_and_price,
        };

        Self { cfg_env, block_env }
    }
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
