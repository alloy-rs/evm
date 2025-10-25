use alloy_consensus::BlockHeader;
use alloy_op_hardforks::OpHardforks;
use op_revm::OpSpecId;

/// Map the latest active hardfork at the given header to a revm [`OpSpecId`].
pub fn spec(chain_spec: impl OpHardforks, header: impl BlockHeader) -> OpSpecId {
    spec_by_timestamp_after_bedrock(chain_spec, header.timestamp())
}

/// Returns the revm [`OpSpecId`] at the given timestamp.
///
/// # Note
///
/// This is only intended to be used after the Bedrock, when hardforks are activated by
/// timestamp.
pub fn spec_by_timestamp_after_bedrock(chain_spec: impl OpHardforks, timestamp: u64) -> OpSpecId {
    if chain_spec.is_interop_active_at_timestamp(timestamp) {
        OpSpecId::INTEROP
    } else if chain_spec.is_jovian_active_at_timestamp(timestamp) {
        OpSpecId::JOVIAN
    } else if chain_spec.is_isthmus_active_at_timestamp(timestamp) {
        OpSpecId::ISTHMUS
    } else if chain_spec.is_holocene_active_at_timestamp(timestamp) {
        OpSpecId::HOLOCENE
    } else if chain_spec.is_granite_active_at_timestamp(timestamp) {
        OpSpecId::GRANITE
    } else if chain_spec.is_fjord_active_at_timestamp(timestamp) {
        OpSpecId::FJORD
    } else if chain_spec.is_ecotone_active_at_timestamp(timestamp) {
        OpSpecId::ECOTONE
    } else if chain_spec.is_canyon_active_at_timestamp(timestamp) {
        OpSpecId::CANYON
    } else if chain_spec.is_regolith_active_at_timestamp(timestamp) {
        OpSpecId::REGOLITH
    } else {
        OpSpecId::BEDROCK
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_consensus::Header;
    use alloy_hardforks::EthereumHardfork;
    use alloy_op_hardforks::{
        EthereumHardforks, ForkCondition, OpChainHardforks, OpHardfork,
        OP_MAINNET_CANYON_TIMESTAMP, OP_MAINNET_ECOTONE_TIMESTAMP, OP_MAINNET_FJORD_TIMESTAMP,
        OP_MAINNET_GRANITE_TIMESTAMP, OP_MAINNET_HOLOCENE_TIMESTAMP, OP_MAINNET_ISTHMUS_TIMESTAMP,
        OP_MAINNET_JOVIAN_TIMESTAMP, OP_MAINNET_REGOLITH_TIMESTAMP,
    };
    use alloy_primitives::BlockTimestamp;

    struct FakeHardfork {
        fork: OpHardfork,
        cond: ForkCondition,
    }

    impl FakeHardfork {
        fn interop() -> Self {
            Self::from_timestamp_zero(OpHardfork::Interop)
        }

        fn jovian() -> Self {
            Self::from_timestamp_zero(OpHardfork::Jovian)
        }

        fn isthmus() -> Self {
            Self::from_timestamp_zero(OpHardfork::Isthmus)
        }

        fn holocene() -> Self {
            Self::from_timestamp_zero(OpHardfork::Holocene)
        }

        fn granite() -> Self {
            Self::from_timestamp_zero(OpHardfork::Granite)
        }

        fn fjord() -> Self {
            Self::from_timestamp_zero(OpHardfork::Fjord)
        }

        fn ecotone() -> Self {
            Self::from_timestamp_zero(OpHardfork::Ecotone)
        }

        fn canyon() -> Self {
            Self::from_timestamp_zero(OpHardfork::Canyon)
        }

        fn regolith() -> Self {
            Self::from_timestamp_zero(OpHardfork::Regolith)
        }

        fn bedrock() -> Self {
            Self::from_block_zero(OpHardfork::Bedrock)
        }

        fn from_block_zero(fork: OpHardfork) -> Self {
            Self { fork, cond: ForkCondition::Block(0) }
        }

        fn from_timestamp_zero(fork: OpHardfork) -> Self {
            Self { fork, cond: ForkCondition::Timestamp(0) }
        }
    }

    impl EthereumHardforks for FakeHardfork {
        fn ethereum_fork_activation(&self, _: EthereumHardfork) -> ForkCondition {
            unimplemented!()
        }
    }

    impl OpHardforks for FakeHardfork {
        fn op_fork_activation(&self, fork: OpHardfork) -> ForkCondition {
            if fork == self.fork {
                self.cond
            } else {
                ForkCondition::Never
            }
        }
    }

    #[test_case::test_case(FakeHardfork::interop(), OpSpecId::INTEROP; "Interop")]
    #[test_case::test_case(FakeHardfork::jovian(), OpSpecId::JOVIAN; "Jovian")]
    #[test_case::test_case(FakeHardfork::isthmus(), OpSpecId::ISTHMUS; "Isthmus")]
    #[test_case::test_case(FakeHardfork::holocene(), OpSpecId::HOLOCENE; "Holocene")]
    #[test_case::test_case(FakeHardfork::granite(), OpSpecId::GRANITE; "Granite")]
    #[test_case::test_case(FakeHardfork::fjord(), OpSpecId::FJORD; "Fjord")]
    #[test_case::test_case(FakeHardfork::ecotone(), OpSpecId::ECOTONE; "Ecotone")]
    #[test_case::test_case(FakeHardfork::canyon(), OpSpecId::CANYON; "Canyon")]
    #[test_case::test_case(FakeHardfork::regolith(), OpSpecId::REGOLITH; "Regolith")]
    #[test_case::test_case(FakeHardfork::bedrock(), OpSpecId::BEDROCK; "Bedrock")]
    fn test_spec_maps_hardfork_successfully(fork: impl OpHardforks, expected_spec: OpSpecId) {
        let header = Header::default();
        let actual_spec = spec(fork, &header);

        assert_eq!(actual_spec, expected_spec);
    }

    #[test_case::test_case(OP_MAINNET_JOVIAN_TIMESTAMP, OpSpecId::JOVIAN; "Jovian")]
    #[test_case::test_case(OP_MAINNET_ISTHMUS_TIMESTAMP, OpSpecId::ISTHMUS; "Isthmus")]
    #[test_case::test_case(OP_MAINNET_HOLOCENE_TIMESTAMP, OpSpecId::HOLOCENE; "Holocene")]
    #[test_case::test_case(OP_MAINNET_GRANITE_TIMESTAMP, OpSpecId::GRANITE; "Granite")]
    #[test_case::test_case(OP_MAINNET_FJORD_TIMESTAMP, OpSpecId::FJORD; "Fjord")]
    #[test_case::test_case(OP_MAINNET_ECOTONE_TIMESTAMP, OpSpecId::ECOTONE; "Ecotone")]
    #[test_case::test_case(OP_MAINNET_CANYON_TIMESTAMP, OpSpecId::CANYON; "Canyon")]
    #[test_case::test_case(OP_MAINNET_REGOLITH_TIMESTAMP, OpSpecId::REGOLITH; "Regolith")]
    fn test_op_spec_maps_hardfork_successfully(timestamp: BlockTimestamp, expected_spec: OpSpecId) {
        let fork = OpChainHardforks::op_mainnet();
        let header = Header { timestamp, ..Default::default() };
        let actual_spec = spec(&fork, &header);

        assert_eq!(actual_spec, expected_spec);
    }
}
