use alloy_consensus::BlockHeader;
use alloy_eips::eip7840::BlobParams;
use alloy_evm::EvmEnv;
use alloy_op_hardforks::OpHardforks;
use alloy_primitives::ChainId;
use op_revm::OpSpecId;

/// Create a new `EvmEnv` from a block `header`, `chain_id`, `chain_spec` and optional
/// `blob_params`.
///
/// # Arguments
///
/// * `header` - The block to make the env out of.
/// * `chain_spec` - The chain hardfork description, must implement [`OpHardforks`].
/// * `chain_id` - The chain identifier.
/// * `blob_params` - Optional parameters that sets limits on gas and count for blobs.
pub fn evm_env_for_op_block(
    header: impl BlockHeader,
    chain_spec: impl OpHardforks,
    chain_id: ChainId,
    blob_params: Option<BlobParams>,
) -> EvmEnv<OpSpecId> {
    EvmEnv::for_block(header, chain_spec, chain_id, blob_params, |c, h| crate::spec(c, h))
}
