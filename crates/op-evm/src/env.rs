use crate::spec;
use alloy_consensus::BlockHeader;
use alloy_eips::eip7840::BlobParams;
use alloy_evm::{eth::spec::EthSpec, EvmEnv};
use alloy_hardforks::{EthereumChainHardforks, EthereumHardforks};
use alloy_op_hardforks::OpChainHardforks;
use alloy_primitives::ChainId;
use op_revm::OpSpecId;
use revm::primitives::hardfork::SpecId;

/// An extension intended for [`EvmEnv`].
pub trait ExtEvmEnv<Spec> {
    /// Create a new `EvmEnv` from a block `header`, `chain_id`, chain `spec` and optional
    /// `blob_params`.
    ///
    /// # Arguments
    ///
    /// * `header` - The block to make the env out of.
    /// * `chain_id` - The chain identifier.
    /// * `spec` - The chain hardfork description, must implement [`EthereumHardforks`] and
    ///   [`DeriveSpec`].
    /// * `blob_params` - Optional parameters that sets limits on gas and count for blobs.
    fn for_block(
        header: impl BlockHeader,
        chain_spec: impl EthereumHardforks + DeriveSpec<SpecId = Spec>,
        chain_id: ChainId,
        blob_params: Option<BlobParams>,
    ) -> Self;
}

impl<Spec> ExtEvmEnv<Spec> for EvmEnv<Spec> {
    /// Create a new `EvmEnv` from a block `header`, `chain_id`, `chain_spec` and optional
    /// `blob_params`.
    ///
    /// # Arguments
    ///
    /// * `header` - The block to make the env out of.
    /// * `chain_spec` - The chain hardfork description, must implement [`EthereumHardforks`] and
    ///   [`DeriveSpec`].
    /// * `chain_id` - The chain identifier.
    /// * `blob_params` - Optional parameters that sets limits on gas and count for blobs.
    fn for_block(
        header: impl BlockHeader,
        chain_spec: impl EthereumHardforks + DeriveSpec<SpecId = Spec>,
        chain_id: ChainId,
        blob_params: Option<BlobParams>,
    ) -> Self {
        Self::for_block(header, chain_spec, chain_id, blob_params, |c, h| c.for_block(h))
    }
}

/// Map the latest active hardfork at the given header to a [`Self::SpecId`].
pub trait DeriveSpec {
    /// An associated spec ID type for a hardfork chain specification.
    type SpecId;

    /// Maps the latest active hardfork at the given header to a [`Self::SpecId`].
    fn for_block(&self, header: impl BlockHeader) -> Self::SpecId;
}

impl DeriveSpec for EthSpec {
    type SpecId = SpecId;

    fn for_block(&self, header: impl BlockHeader) -> Self::SpecId {
        alloy_evm::spec(self, &header)
    }
}

impl DeriveSpec for EthereumChainHardforks {
    type SpecId = SpecId;

    fn for_block(&self, header: impl BlockHeader) -> Self::SpecId {
        alloy_evm::spec(self, &header)
    }
}

impl DeriveSpec for OpChainHardforks {
    type SpecId = OpSpecId;

    fn for_block(&self, header: impl BlockHeader) -> Self::SpecId {
        spec(self, &header)
    }
}
