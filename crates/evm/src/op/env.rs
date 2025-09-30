use crate::{
    eth::{EvmEnvInput, NextEvmEnvAttributes},
    EvmEnv,
};
use alloy_consensus::BlockHeader;
use alloy_op_hardforks::OpHardforks;
use alloy_primitives::{ChainId, U256};
use op_revm::OpSpecId;
use revm::{
    context::{BlockEnv, CfgEnv},
    context_interface::block::BlobExcessGasAndPrice,
    primitives::hardfork::SpecId,
};

impl EvmEnv<OpSpecId> {
    /// Create a new `EvmEnv` with [`OpSpecId`] from a block `header`, `chain_id` and `chain_spec`.
    ///
    /// # Arguments
    ///
    /// * `header` - The block to make the env out of.
    /// * `chain_spec` - The chain hardfork description, must implement [`OpHardforks`].
    /// * `chain_id` - The chain identifier.
    /// * `blob_params` - Optional parameters that sets limits on gas and count for blobs.
    pub fn for_op_block(
        header: impl BlockHeader,
        chain_spec: impl OpHardforks,
        chain_id: ChainId,
    ) -> Self {
        Self::for_op(EvmEnvInput::from_block_header(header), chain_spec, chain_id)
    }

    /// Create a new `EvmEnv` with [`SpecId`] from a parent block `header`, `chain_id` and
    /// `chain_spec`.
    ///
    /// # Arguments
    ///
    /// * `header` - The parent block to make the env out of.
    /// * `base_fee_per_gas` - Base fee per gas for the next block.
    /// * `chain_spec` - The chain hardfork description, must implement [`OpHardforks`].
    /// * `chain_id` - The chain identifier.
    pub fn for_op_next_block(
        header: impl BlockHeader,
        attributes: NextEvmEnvAttributes,
        base_fee_per_gas: u64,
        chain_spec: impl OpHardforks,
        chain_id: ChainId,
    ) -> Self {
        Self::for_op(
            EvmEnvInput::from_parent_header(header, attributes, base_fee_per_gas, 0),
            chain_spec,
            chain_id,
        )
    }

    fn for_op(input: EvmEnvInput, chain_spec: impl OpHardforks, chain_id: ChainId) -> Self {
        let spec = crate::op::spec_by_timestamp_after_bedrock(&chain_spec, input.timestamp);
        let cfg_env = CfgEnv::new().with_chain_id(chain_id).with_spec(spec);

        let blob_excess_gas_and_price = spec
            .into_eth_spec()
            .is_enabled_in(SpecId::CANCUN)
            .then_some(BlobExcessGasAndPrice { excess_blob_gas: 0, blob_gasprice: 1 });

        let is_merge_active = spec.into_eth_spec() >= SpecId::MERGE;

        let block_env = BlockEnv {
            number: U256::from(input.height),
            beneficiary: input.beneficiary,
            timestamp: U256::from(input.timestamp),
            difficulty: if is_merge_active { U256::ZERO } else { input.difficulty },
            prevrandao: if is_merge_active { input.mix_hash } else { None },
            gas_limit: input.gas_limit,
            basefee: input.base_fee_per_gas,
            // EIP-4844 excess blob gas of this block, introduced in Cancun
            blob_excess_gas_and_price,
        };

        Self::new(cfg_env, block_env)
    }
}

#[cfg(feature = "op-engine")]
mod payload {
    use super::*;
    use op_alloy_rpc_types_engine::OpExecutionPayload;

    impl EvmEnv<OpSpecId> {
        /// Create a new `EvmEnv` with [`OpSpecId`] from a `payload`, `chain_id`, `chain_spec` and
        /// optional `blob_params`.
        ///
        /// # Arguments
        ///
        /// * `header` - The block to make the env out of.
        /// * `chain_spec` - The chain hardfork description, must implement [`OpHardforks`].
        /// * `chain_id` - The chain identifier.
        /// * `blob_params` - Optional parameters that sets limits on gas and count for blobs.
        pub fn for_op_payload(
            payload: &OpExecutionPayload,
            chain_spec: impl OpHardforks,
            chain_id: ChainId,
        ) -> Self {
            Self::for_op(EvmEnvInput::from_op_payload(payload), chain_spec, chain_id)
        }
    }

    impl EvmEnvInput {
        pub(crate) fn from_op_payload(payload: &OpExecutionPayload) -> Self {
            Self {
                timestamp: payload.timestamp(),
                height: payload.block_number(),
                beneficiary: payload.as_v1().fee_recipient,
                mix_hash: Some(payload.as_v1().prev_randao),
                difficulty: payload.as_v1().prev_randao.into(),
                gas_limit: payload.as_v1().gas_limit,
                excess_blob_gas: payload.as_v3().map(|v| v.excess_blob_gas),
                base_fee_per_gas: payload.as_v1().base_fee_per_gas.saturating_to(),
                blob_gas_used: None,
                elasticity_multiplier: None,
            }
        }
    }
}
