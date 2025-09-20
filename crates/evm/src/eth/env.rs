use crate::EvmEnv;
use alloy_consensus::BlockHeader;
use alloy_eips::eip7840::BlobParams;
use alloy_hardforks::EthereumHardforks;
use alloy_primitives::{ChainId, U256};
use revm::{
    context::{BlockEnv, CfgEnv},
    context_interface::block::BlobExcessGasAndPrice,
    primitives::hardfork::SpecId,
};

impl EvmEnv<SpecId> {
    /// Create a new `EvmEnv` with [`SpecId`] from a block `header`, `chain_id`, chain `spec` and
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
        /// Maximum transaction gas limit as defined by [EIP-7825](https://eips.ethereum.org/EIPS/eip-7825) activated in `Osaka` hardfork.
        const MAX_TX_GAS_LIMIT_OSAKA: u64 = 2u64.pow(24);

        let spec = crate::eth::spec(&chain_spec, &header);
        let mut cfg_env = CfgEnv::new_with_spec(spec).with_chain_id(chain_id);

        if let Some(blob_params) = &blob_params {
            cfg_env.set_max_blobs_per_tx(blob_params.max_blobs_per_tx);
        }

        if chain_spec.is_osaka_active_at_timestamp(header.timestamp()) {
            cfg_env.tx_gas_limit_cap = Some(MAX_TX_GAS_LIMIT_OSAKA);
        }

        // derive the EIP-4844 blob fees from the header's `excess_blob_gas` and the current
        // blob-params
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

        Self::new(cfg_env, block_env)
    }
}
