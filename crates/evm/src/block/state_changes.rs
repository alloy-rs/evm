//! State changes that are not related to transactions.

use super::{calc, BlockExecutionError};
use alloy_consensus::BlockHeader;
use alloy_eips::eip4895::Withdrawal;
use alloy_hardforks::EthereumHardforks;
use alloy_primitives::{map::AddressMap, U256};
use revm::{
    context::Block,
    state::{Account, EvmState, TransactionId},
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
    withdrawals: Option<&[Withdrawal]>,
) -> AddressMap<u128>
where
    H: BlockHeader,
{
    let mut balance_increments = AddressMap::with_capacity_and_hasher(
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
        withdrawals,
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
) -> AddressMap<u128> {
    let mut balance_increments =
        AddressMap::with_capacity_and_hasher(withdrawals.len(), Default::default());
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
#[inline]
pub fn insert_post_block_withdrawals_balance_increments(
    spec: impl EthereumHardforks,
    block_timestamp: u64,
    withdrawals: Option<&[Withdrawal]>,
    balance_increments: &mut AddressMap<u128>,
) {
    // Process withdrawals
    if spec.is_shanghai_active_at_timestamp(block_timestamp) {
        if let Some(withdrawals) = withdrawals {
            for withdrawal in withdrawals {
                *balance_increments.entry(withdrawal.address).or_default() +=
                    withdrawal.amount_wei().to::<u128>();
            }
        }
    }
}

/// Creates an [`EvmState`] from a map of balance increments and the current state. Loads accounts
/// from the database and applies increments on top of their state.
pub(crate) fn balance_increment_state<DB>(
    balance_increments: &AddressMap<u128>,
    state: &mut DB,
) -> Result<EvmState, BlockExecutionError>
where
    DB: Database,
{
    balance_increments
        .iter()
        .map(|(address, &balance)| {
            let cache_account = state.basic(*address).map_err(|_| {
                BlockExecutionError::msg("could not load account for balance increment")
            })?;

            let mut new_account = cache_account
                .map(Account::from)
                .unwrap_or_else(|| Account::new_not_existing(TransactionId::ZERO));
            new_account.info.balance = new_account.info.balance.saturating_add(U256::from(balance));
            new_account.mark_touch();
            Ok((*address, new_account))
        })
        .collect::<Result<EvmState, _>>()
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{address, Address, U256};
    use revm::{
        database::{CacheDB, EmptyDB, State},
        primitives::{HashMap, KECCAK_EMPTY},
        state::AccountInfo,
    };

    use crate::block::state_changes::balance_increment_state;

    fn setup_state_with_account(
        addr: Address,
        balance: u128,
        nonce: u64,
    ) -> State<CacheDB<EmptyDB>> {
        let db = CacheDB::<EmptyDB>::default();
        let mut state = State::builder().with_database(db).with_bundle_update().build();

        let account_info = AccountInfo {
            balance: U256::from(balance),
            nonce,
            code_hash: KECCAK_EMPTY,
            code: None,
            account_id: None,
        };
        state.insert_account(addr, account_info);
        state
    }

    #[test]
    fn test_balance_increment_state_empty_increments_map() {
        let mut state = State::builder()
            .with_database(CacheDB::<EmptyDB>::default())
            .with_bundle_update()
            .build();

        let increments = HashMap::default();
        let result = balance_increment_state(&increments, &mut state).unwrap();
        assert!(result.is_empty(), "Empty increments map should return empty state");
    }

    #[test]
    fn test_balance_increment_state_multiple_valid_increments() {
        let addr1 = address!("0x1000000000000000000000000000000000000000");
        let addr2 = address!("0x2000000000000000000000000000000000000000");

        let mut state = setup_state_with_account(addr1, 100, 1);

        let account2 = AccountInfo {
            balance: U256::from(200),
            nonce: 1,
            code_hash: KECCAK_EMPTY,
            code: None,
            account_id: None,
        };
        state.insert_account(addr2, account2);

        let mut increments = HashMap::default();
        increments.insert(addr1, 50);
        increments.insert(addr2, 100);

        let result = balance_increment_state(&increments, &mut state).unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result.get(&addr1).unwrap().info.balance, U256::from(150));
        assert_eq!(result.get(&addr2).unwrap().info.balance, U256::from(300));
    }

    #[test]
    fn test_balance_increment_state_mixed_zero_and_nonzero_increments() {
        let addr1 = address!("0x1000000000000000000000000000000000000000");
        let addr2 = address!("0x2000000000000000000000000000000000000000");

        let mut state = setup_state_with_account(addr1, 100, 1);

        let account2 = AccountInfo {
            balance: U256::from(200),
            nonce: 1,
            code_hash: KECCAK_EMPTY,
            code: None,
            account_id: None,
        };
        state.insert_account(addr2, account2);

        let mut increments = HashMap::default();
        increments.insert(addr1, 0);
        increments.insert(addr2, 100);

        let result = balance_increment_state(&increments, &mut state).unwrap();

        assert_eq!(result.get(&addr1).unwrap().info.balance, U256::from(100));
        assert_eq!(result.get(&addr2).unwrap().info.balance, U256::from(300));
    }
}
