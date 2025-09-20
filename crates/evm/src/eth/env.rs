use crate::EvmEnv;
use alloy_consensus::BlockHeader;
use alloy_eips::eip7840::BlobParams;
use alloy_hardforks::EthereumHardforks;
use alloy_primitives::ChainId;
use revm::primitives::hardfork::SpecId;

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
        Self::for_block(header, chain_spec, chain_id, blob_params, crate::eth::spec)
    }
}
