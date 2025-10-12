//! Utility functions for Eip-7928 implementation in Amsterdam and later hardforks.

use alloc::{collections::BTreeMap, vec::Vec};
use alloy_eips::eip7928::{
    balance_change::BalanceChange, code_change::CodeChange, nonce_change::NonceChange,
    AccountChanges, BlockAccessIndex, BlockAccessList, SlotChanges, StorageChange, MAX_CODE_SIZE,
    MAX_TXS_PER_BLOCK,
};
use alloy_primitives::{Address, B256, U256};
use revm::{
    primitives::{StorageKey, StorageValue},
    state::Account,
};

/// An utility function for system contract storage allocation.
pub fn build_post_execution_system_contract_account_change(
    address: Address,
    pre: Vec<StorageValue>,
    post: Vec<StorageValue>,
    tx_index: BlockAccessIndex,
) -> AccountChanges {
    let mut account_changes = AccountChanges::new(address);

    for (i, (pre_val, post_val)) in pre.into_iter().zip(post.into_iter()).enumerate() {
        let slot = StorageKey::from(i as u64);

        if pre_val != post_val {
            let change = StorageChange { block_access_index: tx_index, new_value: post_val.into() };
            account_changes
                .storage_changes
                .push(SlotChanges::default().with_slot(slot.into()).with_change(change));
        } else {
            account_changes.storage_reads.push(slot.into());
        }
    }

    account_changes
}

/// An utility function to build block access list with tx_index.
pub fn from_account_with_tx_index(
    address: Address,
    block_access_index: u64,
    account: &Account,
    initial_balance: U256,
    is_oog: bool,
) -> AccountChanges {
    tracing::debug!("Account  {:?}", account);
    let mut account_changes = AccountChanges::default();
    let final_balance = account.info.balance;
    for key in &account.storage_access.reads {
        tracing::debug!("Storage read at {:#x}: {:#x} ", address, key);
        account_changes.storage_reads.push((*key).into());
    }

    // Group writes by slots
    let mut slot_map: BTreeMap<StorageKey, Vec<StorageChange>> = BTreeMap::new();

    for (slot, (_, post)) in &account.storage_access.writes {
        tracing::debug!("Storage write at {:#x}: {:#x} -> {:#x}", address, slot, post);
        slot_map
            .entry(*slot)
            .or_default()
            .push(StorageChange { block_access_index, new_value: (*post).into() });
    }

    // Convert slot_map into SlotChanges and push into account_changes
    for (slot, changes) in slot_map {
        account_changes.storage_changes.push(SlotChanges { slot: slot.into(), changes });
    }

    // Records if only post_balance != pre_balance
    let (_, post_balance) = account.balance_change;
    tracing::debug!(
        "Balance change at {:#x}: initial: {}, final: {}, post: {}",
        address,
        initial_balance,
        final_balance,
        post_balance
    );
    if initial_balance != post_balance && initial_balance != final_balance {
        account_changes.balance_changes.push(BalanceChange { block_access_index, post_balance });
    }

    let (pre_nonce, post_nonce) = account.nonce_change;
    if pre_nonce != post_nonce {
        account_changes
            .nonce_changes
            .push(NonceChange { block_access_index, new_nonce: post_nonce });
    }

    let (code, modified) = &account.code_change;
    if !code.is_empty() || *modified {
        account_changes
            .code_changes
            .push(CodeChange { block_access_index, new_code: code.clone() });
    }

    account_changes.address = address;
    tracing::debug!("Account Status: {:#x} -> {:#?}", address, account.status);
    if account.is_selfdestructed() || account.is_selfdestructed_locally() {
        tracing::debug!(
            "Account {:#x} was self-destructed. reads: {:?}, writes: {:?}",
            address,
            account_changes.storage_reads,
            account_changes.storage_changes
        );
        account_changes.nonce_changes.clear();
        account_changes.code_changes.clear();

        for slot in &account_changes.storage_changes {
            account_changes.storage_reads.push(slot.slot);
        }
        account_changes.storage_changes.clear();
    }
    if is_oog {
        account_changes.storage_reads.clear();

        account_changes.storage_changes.clear();
    }
    account_changes
}

/// Sort block-level access list and removes duplicates entries by merging them together.
pub fn sort_and_remove_duplicates_in_bal(mut bal: BlockAccessList) -> BlockAccessList {
    tracing::debug!("Bal before sort: {:#?}", bal);
    bal.sort_by_key(|ac| ac.address);
    let mut merged: Vec<AccountChanges> = Vec::new();

    for account in bal {
        if let Some(last) = merged.last_mut() {
            if last.address == account.address {
                // Same address → extend fields
                last.storage_changes.extend(account.storage_changes);
                last.storage_reads.extend(account.storage_reads);
                last.balance_changes.extend(account.balance_changes);
                last.nonce_changes.extend(account.nonce_changes);
                last.code_changes.extend(account.code_changes);
                continue;
            }
        }
        merged.push(account);
    }
    tracing::debug!("Bal after sort: {:#?}", merged);
    merged
}

/// Validates a Block Access List against execution constraints.
pub fn validate_block_access_list_against_execution(block_access_list: &BlockAccessList) -> bool {
    // 1. Validate structural constraints
    for account in block_access_list {
        let changed_slots: alloy_primitives::map::HashSet<_> =
            account.storage_changes.iter().map(|sc| B256::from(sc.slot)).collect();
        let read_slots: alloy_primitives::map::HashSet<_> =
            account.storage_reads.iter().cloned().collect();

        // A slot should not be in both changes and reads (per EIP-7928)
        if !changed_slots.is_disjoint(&read_slots) {
            return false;
        }
    }

    // 2. Validate ordering (addresses should be sorted lexicographically)
    let addresses: Vec<_> = block_access_list.iter().map(|account| account.address).collect();
    let mut sorted_addresses = addresses.clone();
    sorted_addresses.sort();
    if addresses != sorted_addresses {
        return false;
    }

    // 3. Validate all data is within bounds
    let max_block_access_index = MAX_TXS_PER_BLOCK + 1; // 0 for pre-exec, 1..MAX_TXS for txs, MAX_TXS+1 for post-exec
    for account in block_access_list {
        // Validate storage slots are sorted within each account
        let storage_slots: Vec<_> = account.storage_changes.iter().map(|sc| sc.slot).collect();
        let mut sorted_storage_slots = storage_slots.clone();
        sorted_storage_slots.sort();
        if storage_slots != sorted_storage_slots {
            return false;
        }

        // Check storage changes
        for slot_changes in &account.storage_changes {
            // Check changes are sorted by block_access_index
            let indices: Vec<_> =
                slot_changes.changes.iter().map(|c| c.block_access_index).collect();
            let mut sorted_indices = indices.clone();
            sorted_indices.sort();
            if indices != sorted_indices {
                return false;
            }

            for change in &slot_changes.changes {
                if change.block_access_index > max_block_access_index.try_into().unwrap() {
                    return false;
                }
            }
        }

        // Check balance changes are sorted by block_access_index
        let balance_indices: Vec<_> =
            account.balance_changes.iter().map(|bc| bc.block_access_index).collect();
        let mut sorted_balance_indices = balance_indices.clone();
        sorted_balance_indices.sort();
        if balance_indices != sorted_balance_indices {
            return false;
        }

        for balance_change in &account.balance_changes {
            if balance_change.block_access_index > max_block_access_index.try_into().unwrap() {
                return false;
            }
        }

        // Check nonce changes are sorted by block_access_index
        let nonce_indices: Vec<_> =
            account.nonce_changes.iter().map(|nc| nc.block_access_index).collect();
        let mut sorted_nonce_indices = nonce_indices.clone();
        sorted_nonce_indices.sort();
        if nonce_indices != sorted_nonce_indices {
            return false;
        }

        for nonce_change in &account.nonce_changes {
            if nonce_change.block_access_index > max_block_access_index.try_into().unwrap() {
                return false;
            }
        }

        // Check code changes are sorted by block_access_index
        let code_indices: Vec<_> =
            account.code_changes.iter().map(|cc| cc.block_access_index).collect();
        let mut sorted_code_indices = code_indices.clone();
        sorted_code_indices.sort();
        if code_indices != sorted_code_indices {
            return false;
        }

        for code_change in &account.code_changes {
            if code_change.block_access_index > max_block_access_index.try_into().unwrap() {
                return false;
            }
            if code_change.new_code.len() > MAX_CODE_SIZE {
                return false;
            }
        }
    }

    true
}

/// Validate the block access list against an expected block access list
pub fn validate_block_access_list(
    block_access_list: &BlockAccessList,
    expected_block_access_list: &BlockAccessList,
) -> bool {
    if alloy_primitives::keccak256(alloy_rlp::encode(block_access_list))
        != alloy_primitives::keccak256(alloy_rlp::encode(expected_block_access_list))
    {
        return false;
    }
    true
}
#[cfg(test)]
mod tests {
    use alloy_eips::eip7928::CodeChange;

    #[test]
    fn it_works() {
        let c = CodeChange::default().new_code;
        println!("{:?}", c);
        println!("{:?}", c.len());
        println!("{:?}", c.is_empty());
    }
}
