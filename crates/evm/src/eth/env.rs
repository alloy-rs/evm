use crate::EvmEnv;
use alloy_consensus::BlockHeader;
use alloy_eips::{eip7825::MAX_TX_GAS_LIMIT_OSAKA, eip7840::BlobParams};
use alloy_hardforks::EthereumHardforks;
use alloy_primitives::{Address, BlockNumber, BlockTimestamp, ChainId, B256, U256};
use revm::{
    context::{BlockEnv, CfgEnv},
    context_interface::block::BlobExcessGasAndPrice,
    primitives::hardfork::SpecId,
};

impl EvmEnv<SpecId> {
    /// Create a new `EvmEnv` with [`SpecId`] from a block `header`, `chain_id`, `chain_spec` and
    /// optional `blob_params`.
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
        Self::for_eth(EvmEnvInput::from_block_header(header), chain_spec, chain_id, blob_params)
    }

    fn for_eth(
        input: EvmEnvInput,
        chain_spec: impl EthereumHardforks,
        chain_id: ChainId,
        blob_params: Option<BlobParams>,
    ) -> Self {
        let spec = crate::eth::spec_by_timestamp_and_block_number(
            &chain_spec,
            input.timestamp,
            input.height,
        );
        let mut cfg_env = CfgEnv::new_with_spec(spec).with_chain_id(chain_id);

        if let Some(blob_params) = &blob_params {
            cfg_env.set_max_blobs_per_tx(blob_params.max_blobs_per_tx);
        }

        if chain_spec.is_osaka_active_at_timestamp(input.timestamp) {
            cfg_env.tx_gas_limit_cap = Some(MAX_TX_GAS_LIMIT_OSAKA);
        }

        // derive the EIP-4844 blob fees from the header's `excess_blob_gas` and the current
        // blob-params
        let blob_excess_gas_and_price =
            input.excess_blob_gas.zip(blob_params).map(|(excess_blob_gas, params)| {
                let blob_gasprice = params.calc_blob_fee(excess_blob_gas);
                BlobExcessGasAndPrice { excess_blob_gas, blob_gasprice }
            });

        let is_merge_active = chain_spec.is_paris_active_at_block(input.height);

        let block_env = BlockEnv {
            number: U256::from(input.height),
            beneficiary: input.beneficiary,
            timestamp: U256::from(input.timestamp),
            difficulty: if is_merge_active { U256::ZERO } else { input.difficulty },
            prevrandao: if is_merge_active { input.mix_hash } else { None },
            gas_limit: input.gas_limit,
            basefee: input.base_fee_per_gas,
            blob_excess_gas_and_price,
        };

        Self::new(cfg_env, block_env)
    }
}

pub(crate) struct EvmEnvInput {
    pub(crate) timestamp: BlockTimestamp,
    pub(crate) height: BlockNumber,
    pub(crate) beneficiary: Address,
    pub(crate) mix_hash: Option<B256>,
    pub(crate) difficulty: U256,
    pub(crate) gas_limit: u64,
    pub(crate) excess_blob_gas: Option<u64>,
    pub(crate) base_fee_per_gas: u64,
}

impl EvmEnvInput {
    pub(crate) fn from_block_header(header: impl BlockHeader) -> Self {
        Self {
            timestamp: header.timestamp(),
            height: header.number(),
            beneficiary: header.beneficiary(),
            mix_hash: header.mix_hash(),
            difficulty: header.difficulty(),
            gas_limit: header.gas_limit(),
            excess_blob_gas: header.excess_blob_gas(),
            base_fee_per_gas: header.base_fee_per_gas().unwrap_or_default(),
        }
    }
}

#[cfg(feature = "engine")]
mod payload {
    use super::*;
    use alloy_rpc_types_engine::ExecutionPayload;

    impl EvmEnv<SpecId> {
        /// Create a new `EvmEnv` with [`SpecId`] from a `payload`, `chain_id`, `chain_spec` and
        /// optional `blob_params`.
        ///
        /// # Arguments
        ///
        /// * `header` - The block to make the env out of.
        /// * `chain_spec` - The chain hardfork description, must implement [`EthereumHardforks`].
        /// * `chain_id` - The chain identifier.
        /// * `blob_params` - Optional parameters that sets limits on gas and count for blobs.
        pub fn for_eth_payload(
            payload: &ExecutionPayload,
            chain_spec: impl EthereumHardforks,
            chain_id: ChainId,
            blob_params: Option<BlobParams>,
        ) -> Self {
            Self::for_eth(EvmEnvInput::from_payload(payload), chain_spec, chain_id, blob_params)
        }
    }

    impl EvmEnvInput {
        pub(crate) fn from_payload(payload: &ExecutionPayload) -> Self {
            Self {
                timestamp: payload.timestamp(),
                height: payload.block_number(),
                beneficiary: payload.fee_recipient(),
                mix_hash: Some(payload.as_v1().prev_randao),
                difficulty: payload.as_v1().prev_randao.into(),
                gas_limit: payload.gas_limit(),
                excess_blob_gas: payload.excess_blob_gas(),
                base_fee_per_gas: payload.saturated_base_fee_per_gas(),
            }
        }
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
