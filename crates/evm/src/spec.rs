use alloy_consensus::BlockHeader;
use alloy_hardforks::EthereumHardforks;
use revm::primitives::hardfork::SpecId;

/// Map the latest active hardfork at the given header to a [`SpecId`].
pub fn spec<C, H>(chain_spec: &C, header: &H) -> SpecId
where
    C: EthereumHardforks,
    H: BlockHeader,
{
    spec_by_timestamp_and_block_number(chain_spec, header.timestamp(), header.number())
}

/// Map the latest active hardfork at the given timestamp or block number to a [`SpecId`].
pub fn spec_by_timestamp_and_block_number<C>(
    chain_spec: &C,
    timestamp: u64,
    block_number: u64,
) -> SpecId
where
    C: EthereumHardforks,
{
    if chain_spec.is_osaka_active_at_timestamp(timestamp) {
        SpecId::OSAKA
    } else if chain_spec.is_prague_active_at_timestamp(timestamp) {
        SpecId::PRAGUE
    } else if chain_spec.is_cancun_active_at_timestamp(timestamp) {
        SpecId::CANCUN
    } else if chain_spec.is_shanghai_active_at_timestamp(timestamp) {
        SpecId::SHANGHAI
    } else if chain_spec.is_paris_active_at_block(block_number) {
        SpecId::MERGE
    } else if chain_spec.is_london_active_at_block(block_number) {
        SpecId::LONDON
    } else if chain_spec.is_berlin_active_at_block(block_number) {
        SpecId::BERLIN
    } else if chain_spec.is_istanbul_active_at_block(block_number) {
        SpecId::ISTANBUL
    } else if chain_spec.is_petersburg_active_at_block(block_number) {
        SpecId::PETERSBURG
    } else if chain_spec.is_byzantium_active_at_block(block_number) {
        SpecId::BYZANTIUM
    } else if chain_spec.is_spurious_dragon_active_at_block(block_number) {
        SpecId::SPURIOUS_DRAGON
    } else if chain_spec.is_tangerine_whistle_active_at_block(block_number) {
        SpecId::TANGERINE
    } else if chain_spec.is_homestead_active_at_block(block_number) {
        SpecId::HOMESTEAD
    } else {
        SpecId::FRONTIER
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eth::spec::EthSpec;
    use alloy_consensus::Header;
    use alloy_hardforks::{EthereumHardfork, ForkCondition};
    use alloy_primitives::{BlockNumber, BlockTimestamp, U256};

    struct FakeHardfork {
        fork: EthereumHardfork,
        cond: ForkCondition,
    }

    impl FakeHardfork {
        fn cancun() -> Self {
            Self::from_timestamp_zero(EthereumHardfork::Cancun)
        }

        fn shanghai() -> Self {
            Self::from_timestamp_zero(EthereumHardfork::Shanghai)
        }

        fn paris() -> Self {
            Self::from_block_zero(EthereumHardfork::Paris)
        }

        fn london() -> Self {
            Self::from_block_zero(EthereumHardfork::London)
        }

        fn berlin() -> Self {
            Self::from_block_zero(EthereumHardfork::Berlin)
        }

        fn istanbul() -> Self {
            Self::from_block_zero(EthereumHardfork::Istanbul)
        }

        fn petersburg() -> Self {
            Self::from_block_zero(EthereumHardfork::Petersburg)
        }

        fn spurious_dragon() -> Self {
            Self::from_block_zero(EthereumHardfork::SpuriousDragon)
        }

        fn homestead() -> Self {
            Self::from_block_zero(EthereumHardfork::Homestead)
        }

        fn frontier() -> Self {
            Self::from_block_zero(EthereumHardfork::Frontier)
        }

        fn from_block_zero(fork: EthereumHardfork) -> Self {
            Self { fork, cond: ForkCondition::Block(0) }
        }

        fn from_timestamp_zero(fork: EthereumHardfork) -> Self {
            Self { fork, cond: ForkCondition::Timestamp(0) }
        }
    }

    impl EthereumHardforks for FakeHardfork {
        fn ethereum_fork_activation(&self, fork: EthereumHardfork) -> ForkCondition {
            if fork == self.fork {
                self.cond
            } else {
                ForkCondition::Never
            }
        }
    }

    #[test_case::test_case(FakeHardfork::cancun(), 0, SpecId::CANCUN; "Cancun")]
    #[test_case::test_case(FakeHardfork::shanghai(), 0, SpecId::SHANGHAI; "Shanghai")]
    #[test_case::test_case(EthSpec::mainnet(), 15537394, SpecId::MERGE; "Paris")]
    fn test_spec_maps_timestamp_successfully(
        fork: impl EthereumHardforks,
        block_number: BlockNumber,
        expected_spec: SpecId,
    ) {
        let actual_spec = spec_by_timestamp_and_block_number(&fork, 0, block_number);

        assert_eq!(actual_spec, expected_spec);
    }

    #[test_case::test_case(FakeHardfork::cancun(), SpecId::CANCUN; "Cancun")]
    #[test_case::test_case(FakeHardfork::shanghai(), SpecId::SHANGHAI; "Shanghai")]
    #[test_case::test_case(FakeHardfork::paris(), SpecId::MERGE; "Merge")]
    #[test_case::test_case(FakeHardfork::london(), SpecId::LONDON; "London")]
    #[test_case::test_case(FakeHardfork::berlin(), SpecId::BERLIN; "Berlin")]
    #[test_case::test_case(FakeHardfork::istanbul(), SpecId::ISTANBUL; "Istanbul")]
    #[test_case::test_case(FakeHardfork::petersburg(), SpecId::PETERSBURG; "Petersburg")]
    #[test_case::test_case(FakeHardfork::spurious_dragon(), SpecId::SPURIOUS_DRAGON; "Spurious dragon")]
    #[test_case::test_case(FakeHardfork::homestead(), SpecId::HOMESTEAD; "Homestead")]
    #[test_case::test_case(FakeHardfork::frontier(), SpecId::FRONTIER; "Frontier")]
    fn test_spec_maps_hardfork_successfully(fork: impl EthereumHardforks, expected_spec: SpecId) {
        let actual_spec = spec(&fork, &Header::default());

        assert_eq!(actual_spec, expected_spec);
    }

    #[test_case::test_case(1710338135, 0, U256::ZERO, SpecId::CANCUN; "Cancun")]
    #[test_case::test_case(1681338455, 0, U256::ZERO, SpecId::SHANGHAI; "Shanghai")]
    #[test_case::test_case(0, 15537394, U256::from(10_u128), SpecId::MERGE; "Merge")]
    #[test_case::test_case(0, 15537394 - 10, U256::ZERO, SpecId::LONDON; "London")]
    #[test_case::test_case(0, 12244000 + 10, U256::ZERO, SpecId::BERLIN; "Berlin")]
    #[test_case::test_case(0, 12244000 - 10, U256::ZERO, SpecId::ISTANBUL; "Istanbul")]
    #[test_case::test_case(0, 7280000 + 10, U256::ZERO, SpecId::PETERSBURG; "Petersburg")]
    #[test_case::test_case(0, 7280000 - 10, U256::ZERO, SpecId::BYZANTIUM; "Byzantium")]
    #[test_case::test_case(0, 2675000 + 10, U256::ZERO, SpecId::SPURIOUS_DRAGON; "Spurious dragon")]
    #[test_case::test_case(0, 2675000 - 10, U256::ZERO, SpecId::TANGERINE; "Tangerine")]
    #[test_case::test_case(0, 1150000 + 10, U256::ZERO, SpecId::HOMESTEAD; "Homestead")]
    #[test_case::test_case(0, 1150000 - 10, U256::ZERO, SpecId::FRONTIER; "Frontier")]
    fn test_spec_maps_header_successfully(
        timestamp: BlockTimestamp,
        number: BlockNumber,
        difficulty: U256,
        expected_spec: SpecId,
    ) {
        let header = Header { timestamp, number, difficulty, ..Default::default() };
        let actual_spec = spec(&EthSpec::mainnet(), &header);

        assert_eq!(actual_spec, expected_spec);
    }
}
