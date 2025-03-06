//! State changes that are not related to transactions.

use super::calc;
use alloy_consensus::BlockHeader;
use alloy_eips::eip4895::{Withdrawal, Withdrawals};
use alloy_hardforks::EthereumHardforks;
use alloy_primitives::{map::HashMap, Address};
use revm::context::BlockEnv;

/// Collect all balance changes at the end of the block.
///
/// Balance changes might include the block reward, uncle rewards, withdrawals, or irregular
/// state changes (DAO fork).
#[inline]
pub fn post_block_balance_increments<H>(
    chain_spec: impl EthereumHardforks,
    block_env: &BlockEnv,
    ommers: &[H],
    withdrawals: Option<&Withdrawals>,
) -> HashMap<Address, u128>
where
    H: BlockHeader,
{
    let mut balance_increments = HashMap::default();

    // Add block rewards if they are enabled.
    if let Some(base_block_reward) = calc::base_block_reward(&chain_spec, block_env.number) {
        // Ommer rewards
        for ommer in ommers {
            *balance_increments.entry(ommer.beneficiary()).or_default() +=
                calc::ommer_reward(base_block_reward, block_env.number, ommer.number());
        }

        // Full block reward
        *balance_increments.entry(block_env.beneficiary).or_default() +=
            calc::block_reward(base_block_reward, ommers.len());
    }

    // process withdrawals
    insert_post_block_withdrawals_balance_increments(
        chain_spec,
        block_env.timestamp,
        withdrawals.map(|w| w.as_slice()),
        &mut balance_increments,
    );

    balance_increments
}

/// Returns a map of addresses to their balance increments if the Shanghai hardfork is active at the
/// given timestamp.
///
/// Zero-valued withdrawals are filtered out.
#[inline]
pub fn post_block_withdrawals_balance_increments<ChainSpec: EthereumHardforks>(
    chain_spec: &ChainSpec,
    block_timestamp: u64,
    withdrawals: &[Withdrawal],
) -> HashMap<Address, u128> {
    let mut balance_increments =
        HashMap::with_capacity_and_hasher(withdrawals.len(), Default::default());
    insert_post_block_withdrawals_balance_increments(
        chain_spec,
        block_timestamp,
        Some(withdrawals),
        &mut balance_increments,
    );
    balance_increments
}

/// Applies all withdrawal balance increments if shanghai is active at the given timestamp to the
/// given `balance_increments` map.
///
/// Zero-valued withdrawals are filtered out.
#[inline]
pub fn insert_post_block_withdrawals_balance_increments(
    chain_spec: impl EthereumHardforks,
    block_timestamp: u64,
    withdrawals: Option<&[Withdrawal]>,
    balance_increments: &mut HashMap<Address, u128>,
) {
    // Process withdrawals
    if chain_spec.is_shanghai_active_at_timestamp(block_timestamp) {
        if let Some(withdrawals) = withdrawals {
            for withdrawal in withdrawals {
                if withdrawal.amount > 0 {
                    *balance_increments.entry(withdrawal.address).or_default() +=
                        withdrawal.amount_wei().to::<u128>();
                }
            }
        }
    }
}
