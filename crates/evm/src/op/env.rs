use crate::EvmEnv;
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
    /// Create a new `EvmEnv` with [`OpSpecId`] from a block `header`, `chain_id`, chain `spec` and
    /// optional `blob_params`.
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
        let spec = crate::op::spec(&chain_spec, &header);
        let cfg_env = CfgEnv::new().with_chain_id(chain_id).with_spec(spec);

        let blob_excess_gas_and_price = spec
            .into_eth_spec()
            .is_enabled_in(SpecId::CANCUN)
            .then_some(BlobExcessGasAndPrice { excess_blob_gas: 0, blob_gasprice: 1 });

        let block_env = BlockEnv {
            number: U256::from(header.number()),
            beneficiary: header.beneficiary(),
            timestamp: U256::from(header.timestamp()),
            difficulty: if spec.into_eth_spec() >= SpecId::MERGE {
                U256::ZERO
            } else {
                header.difficulty()
            },
            prevrandao: if spec.into_eth_spec() >= SpecId::MERGE {
                header.mix_hash()
            } else {
                None
            },
            gas_limit: header.gas_limit(),
            basefee: header.base_fee_per_gas().unwrap_or_default(),
            // EIP-4844 excess blob gas of this block, introduced in Cancun
            blob_excess_gas_and_price,
        };

        Self::new(cfg_env, block_env)
    }
}
