use alloy_consensus::BlockHeader;
use alloy_hardforks::EthereumHardforks;
use alloy_primitives::{BlockNumber, BlockTimestamp};
use revm::primitives::hardfork::SpecId;

/// Map the latest active hardfork to a [`SpecId`].
///
/// The latest active hardfork is determined based on identifying block header attributes. There are
/// several methods to use:
/// * [`spec_for_header()`] to create [`SpecId`] based on a [`BlockHeader`].
/// * [`spec_at_timestamp()`] to create [`SpecId`] based on a [`BlockTimestamp`] if possible.
/// * [`spec_at_height()`] to create [`SpecId`] based on a [`BlockNumber`] if possible.
/// * [`spec_at()`] to create [`SpecId`] based on all possible block identifiers used for hardfork
///   activation.
///
/// It is not intended for this trait to be implemented manually. Instead, any [`EthereumHardforks`]
/// gets `SpecMap` implementation automatically thanks to a blanket implementation.
///
/// [`spec_for_header()`]: Self::spec_for_header
/// [`spec_at_timestamp()`]: Self::spec_at_timestamp
/// [`spec_at_height()`]: Self::spec_at_height
/// [`spec_at()`]: Self::spec_at
pub trait SpecMap {
    /// Map the latest active hardfork for `header` to a [`SpecId`].
    fn spec_for_header(&self, header: &impl BlockHeader) -> SpecId {
        self.spec_at(header.timestamp(), header.number())
    }

    /// Map the latest active hardfork at `timestamp` to a [`SpecId`].
    fn spec_at_timestamp(&self, timestamp: BlockTimestamp) -> Option<SpecId>;

    /// Map the latest active hardfork at `height` to a [`SpecId`].
    fn spec_at_height(&self, height: BlockNumber) -> Option<SpecId>;

    /// Map the latest active hardfork at `timestamp` or `height` to a [`SpecId`].
    fn spec_at(&self, timestamp: BlockTimestamp, height: BlockNumber) -> SpecId {
        self.spec_at_timestamp(timestamp)
            .or_else(|| self.spec_at_height(height))
            .unwrap_or(SpecId::FRONTIER)
    }
}

impl<C: EthereumHardforks> SpecMap for C {
    fn spec_at_timestamp(&self, timestamp: BlockTimestamp) -> Option<SpecId> {
        Some(if self.is_osaka_active_at_timestamp(timestamp) {
            SpecId::OSAKA
        } else if self.is_prague_active_at_timestamp(timestamp) {
            SpecId::PRAGUE
        } else if self.is_cancun_active_at_timestamp(timestamp) {
            SpecId::CANCUN
        } else if self.is_shanghai_active_at_timestamp(timestamp) {
            SpecId::SHANGHAI
        } else {
            return None;
        })
    }

    fn spec_at_height(&self, height: BlockNumber) -> Option<SpecId> {
        Some(if self.is_paris_active_at_block(height) {
            SpecId::MERGE
        } else if self.is_london_active_at_block(height) {
            SpecId::LONDON
        } else if self.is_berlin_active_at_block(height) {
            SpecId::BERLIN
        } else if self.is_istanbul_active_at_block(height) {
            SpecId::ISTANBUL
        } else if self.is_petersburg_active_at_block(height) {
            SpecId::PETERSBURG
        } else if self.is_byzantium_active_at_block(height) {
            SpecId::BYZANTIUM
        } else if self.is_spurious_dragon_active_at_block(height) {
            SpecId::SPURIOUS_DRAGON
        } else if self.is_tangerine_whistle_active_at_block(height) {
            SpecId::TANGERINE
        } else if self.is_homestead_active_at_block(height) {
            SpecId::HOMESTEAD
        } else {
            return None;
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eth::spec::EthSpec;
    use alloy_consensus::Header;
    use alloy_hardforks::{
        mainnet::{
            MAINNET_BERLIN_BLOCK, MAINNET_BYZANTIUM_BLOCK, MAINNET_CANCUN_TIMESTAMP,
            MAINNET_FRONTIER_BLOCK, MAINNET_HOMESTEAD_BLOCK, MAINNET_ISTANBUL_BLOCK,
            MAINNET_LONDON_BLOCK, MAINNET_PARIS_BLOCK, MAINNET_PETERSBURG_BLOCK,
            MAINNET_PRAGUE_TIMESTAMP, MAINNET_SHANGHAI_TIMESTAMP, MAINNET_SPURIOUS_DRAGON_BLOCK,
            MAINNET_TANGERINE_BLOCK,
        },
        EthereumHardfork, ForkCondition,
    };
    use alloy_primitives::{BlockNumber, BlockTimestamp};

    struct FakeHardfork {
        fork: EthereumHardfork,
        cond: ForkCondition,
    }

    impl FakeHardfork {
        fn osaka() -> Self {
            Self::from_timestamp_zero(EthereumHardfork::Osaka)
        }

        fn prague() -> Self {
            Self::from_timestamp_zero(EthereumHardfork::Prague)
        }

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

    #[test_case::test_case(FakeHardfork::osaka(), SpecId::OSAKA; "Osaka")]
    #[test_case::test_case(FakeHardfork::prague(), SpecId::PRAGUE; "Prague")]
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
        let header = Header::default();
        let actual_spec = fork.spec_for_header(&header);

        assert_eq!(actual_spec, expected_spec);
    }

    #[test_case::test_case(MAINNET_PRAGUE_TIMESTAMP, 0, SpecId::PRAGUE; "Prague")]
    #[test_case::test_case(MAINNET_CANCUN_TIMESTAMP, 0, SpecId::CANCUN; "Cancun")]
    #[test_case::test_case(MAINNET_SHANGHAI_TIMESTAMP, 0, SpecId::SHANGHAI; "Shanghai")]
    #[test_case::test_case(0, MAINNET_PARIS_BLOCK, SpecId::MERGE; "Merge")]
    #[test_case::test_case(0, MAINNET_LONDON_BLOCK, SpecId::LONDON; "London")]
    #[test_case::test_case(0, MAINNET_BERLIN_BLOCK, SpecId::BERLIN; "Berlin")]
    #[test_case::test_case(0, MAINNET_ISTANBUL_BLOCK, SpecId::ISTANBUL; "Istanbul")]
    #[test_case::test_case(0, MAINNET_PETERSBURG_BLOCK, SpecId::PETERSBURG; "Petersburg")]
    #[test_case::test_case(0, MAINNET_BYZANTIUM_BLOCK, SpecId::BYZANTIUM; "Byzantium")]
    #[test_case::test_case(0, MAINNET_SPURIOUS_DRAGON_BLOCK, SpecId::SPURIOUS_DRAGON; "Spurious dragon")]
    #[test_case::test_case(0, MAINNET_TANGERINE_BLOCK, SpecId::TANGERINE; "Tangerine")]
    #[test_case::test_case(0, MAINNET_HOMESTEAD_BLOCK, SpecId::HOMESTEAD; "Homestead")]
    #[test_case::test_case(0, MAINNET_FRONTIER_BLOCK, SpecId::FRONTIER; "Frontier")]
    fn test_eth_spec_maps_hardfork_successfully(
        timestamp: BlockTimestamp,
        number: BlockNumber,
        expected_spec: SpecId,
    ) {
        let fork = EthSpec::mainnet();
        let header = Header { timestamp, number, ..Default::default() };
        let actual_spec = fork.spec_for_header(&header);

        assert_eq!(actual_spec, expected_spec);
    }
}
