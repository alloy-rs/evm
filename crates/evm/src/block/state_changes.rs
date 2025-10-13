//! State changes that are not related to transactions.

use super::{calc, BlockExecutionError};
use alloy_consensus::BlockHeader;
use alloy_eips::eip4895::{Withdrawal, Withdrawals};
use alloy_hardforks::EthereumHardforks;
use alloy_primitives::{map::HashMap, Address};
use revm::{
    context::Block,
    database::State,
    state::{Account, AccountStatus, EvmState},
    Database,
};

/// Collect all balance changes at the end of the block.
///
/// Balance changes might include the block reward, uncle rewards, withdrawals, or irregular
/// state changes (DAO fork).
#[inline]
pub fn post_block_balance_increments<H>(
    spec: impl EthereumHardforks,
    block_env: impl Block,
    ommers: &[H],
    withdrawals: Option<&Withdrawals>,
) -> HashMap<Address, u128>
where
    H: BlockHeader,
{
    let mut balance_increments = HashMap::with_capacity_and_hasher(
        withdrawals.map_or(ommers.len(), |w| w.len()),
        Default::default(),
    );

    // Add block rewards if they are enabled.
    if let Some(base_block_reward) =
        calc::base_block_reward(&spec, block_env.number().saturating_to())
    {
        // Ommer rewards
        for ommer in ommers {
            *balance_increments.entry(ommer.beneficiary()).or_default() += calc::ommer_reward(
                base_block_reward,
                block_env.number().saturating_to(),
                ommer.number(),
            );
        }

        // Full block reward
        *balance_increments.entry(block_env.beneficiary()).or_default() +=
            calc::block_reward(base_block_reward, ommers.len());
    }

    // process withdrawals
    insert_post_block_withdrawals_balance_increments(
        spec,
        block_env.timestamp().saturating_to(),
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
pub fn post_block_withdrawals_balance_increments(
    spec: impl EthereumHardforks,
    block_timestamp: u64,
    withdrawals: &[Withdrawal],
) -> HashMap<Address, u128> {
    let mut balance_increments =
        HashMap::with_capacity_and_hasher(withdrawals.len(), Default::default());
    insert_post_block_withdrawals_balance_increments(
        spec,
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
    spec: impl EthereumHardforks,
    block_timestamp: u64,
    withdrawals: Option<&[Withdrawal]>,
    balance_increments: &mut HashMap<Address, u128>,
) {
    // Process withdrawals
    if spec.is_shanghai_active_at_timestamp(block_timestamp) {
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

/// Creates an `EvmState` from a map of balance increments and the current state
/// to load accounts from. No balance increment is done in the function.
/// Zero balance increments are ignored and won't create state entries.
pub fn balance_increment_state<DB>(
    balance_increments: &HashMap<Address, u128>,
    state: &mut State<DB>,
) -> Result<EvmState, BlockExecutionError>
where
    DB: Database,
{
    let mut load_account = |address: &Address| -> Result<(Address, Account), BlockExecutionError> {
        let cache_account = state.load_cache_account(*address).map_err(|_| {
            BlockExecutionError::msg("could not load account for balance increment")
        })?;

        let account = cache_account.account.as_ref().ok_or_else(|| {
            BlockExecutionError::msg("could not load account for balance increment")
        })?;

        Ok((
            *address,
            Account {
                info: account.info.clone(),
                storage: Default::default(),
                status: AccountStatus::Touched,
                transaction_id: 0,
            },
        ))
    };

    balance_increments
        .iter()
        .filter(|(_, &balance)| balance != 0)
        .map(|(addr, _)| load_account(addr))
        .collect::<Result<EvmState, _>>()
}
