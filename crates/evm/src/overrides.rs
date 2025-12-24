//! RPC utilities for working with EVM.
//!
//! This module provides helper functions for RPC implementations, including:
//! - Block and state overrides

use alloc::{boxed::Box, collections::BTreeMap};
use alloy_primitives::{keccak256, map::HashMap, Address, B256, U256};
use alloy_rpc_types_eth::{
    state::{AccountOverride, StateOverride},
    BlockOverrides,
};
use revm::{
    bytecode::BytecodeDecodeError,
    context::BlockEnv,
    database::{CacheDB, State},
    state::{Account, AccountStatus, Bytecode, EvmStorageSlot},
    Database, DatabaseCommit,
};

/// Errors that can occur when applying state overrides.
#[derive(Debug, thiserror::Error)]
pub enum StateOverrideError<E> {
    /// Invalid bytecode provided in override.
    #[error(transparent)]
    InvalidBytecode(#[from] BytecodeDecodeError),
    /// Both state and state_diff were provided for an account.
    #[error("Both 'state' and 'stateDiff' fields are set for account {0}")]
    BothStateAndStateDiff(Address),
    /// Database error occurred.
    #[error(transparent)]
    Database(E),
}

/// Helper trait implemented for databases that support overriding block hashes.
///
/// Used for applying [`BlockOverrides::block_hash`]
pub trait OverrideBlockHashes {
    /// Overrides the given block hashes.
    fn override_block_hashes(&mut self, block_hashes: BTreeMap<u64, B256>);

    /// Applies the given block overrides to the env and updates overridden block hashes.
    fn apply_block_overrides(&mut self, overrides: BlockOverrides, env: &mut BlockEnv)
    where
        Self: Sized,
    {
        apply_block_overrides(overrides, self, env);
    }
}

impl<DB> OverrideBlockHashes for CacheDB<DB> {
    fn override_block_hashes(&mut self, block_hashes: BTreeMap<u64, B256>) {
        self.cache
            .block_hashes
            .extend(block_hashes.into_iter().map(|(num, hash)| (U256::from(num), hash)))
    }
}

impl<DB> OverrideBlockHashes for State<DB> {
    fn override_block_hashes(&mut self, block_hashes: BTreeMap<u64, B256>) {
        self.block_hashes.extend(block_hashes);
    }
}

/// Applies the given block overrides to the env and updates overridden block hashes in the db.
pub fn apply_block_overrides<DB>(overrides: BlockOverrides, db: &mut DB, env: &mut BlockEnv)
where
    DB: OverrideBlockHashes,
{
    let BlockOverrides {
        number,
        difficulty,
        time,
        gas_limit,
        coinbase,
        random,
        base_fee,
        block_hash,
    } = overrides;

    if let Some(block_hashes) = block_hash {
        // override block hashes
        db.override_block_hashes(block_hashes);
    }

    if let Some(number) = number {
        env.number = number.saturating_to();
    }
    if let Some(difficulty) = difficulty {
        env.difficulty = difficulty;
    }
    if let Some(time) = time {
        env.timestamp = U256::from(time);
    }
    if let Some(gas_limit) = gas_limit {
        env.gas_limit = gas_limit;
    }
    if let Some(coinbase) = coinbase {
        env.beneficiary = coinbase;
    }
    if let Some(random) = random {
        env.prevrandao = Some(random);
    }
    if let Some(base_fee) = base_fee {
        env.basefee = base_fee.saturating_to();
    }
}

/// Applies the given state overrides (a set of [`AccountOverride`]) to the database.
pub fn apply_state_overrides<DB>(
    overrides: StateOverride,
    db: &mut DB,
) -> Result<(), StateOverrideError<DB::Error>>
where
    DB: Database + DatabaseCommit,
{
    for (account, account_overrides) in overrides {
        apply_account_override(account, account_overrides, db)?;
    }
    Ok(())
}

/// Applies a single [`AccountOverride`] to the database.
fn apply_account_override<DB>(
    account: Address,
    account_override: AccountOverride,
    db: &mut DB,
) -> Result<(), StateOverrideError<DB::Error>>
where
    DB: Database + DatabaseCommit,
{
    let mut info = db.basic(account).map_err(StateOverrideError::Database)?.unwrap_or_default();

    if let Some(nonce) = account_override.nonce {
        info.nonce = nonce;
    }
    if let Some(code) = account_override.code {
        // we need to set both the bytecode and the codehash
        info.code_hash = keccak256(&code);
        info.code = Some(Bytecode::new_raw_checked(code)?);
    }
    if let Some(balance) = account_override.balance {
        info.balance = balance;
    }

    // Create a new account marked as touched
    let mut acc = revm::state::Account {
        info: info.clone(),
        original_info: Box::new(info),
        status: AccountStatus::Touched,
        storage: Default::default(),
        transaction_id: 0,
    };

    let storage_diff = match (account_override.state, account_override.state_diff) {
        (Some(_), Some(_)) => return Err(StateOverrideError::BothStateAndStateDiff(account)),
        (None, None) => None,
        // If we need to override the entire state, we firstly mark account as destroyed to clear
        // its storage, and then we mark it is "NewlyCreated" to make sure that old storage won't be
        // used.
        (Some(state), None) => {
            // Destroy the account to ensure that its storage is cleared
            db.commit(HashMap::from_iter([(
                account,
                Account {
                    status: AccountStatus::SelfDestructed | AccountStatus::Touched,
                    ..Default::default()
                },
            )]));
            // Mark the account as created to ensure that old storage is not read
            acc.mark_created();
            Some(state)
        }
        (None, Some(state)) => Some(state),
    };

    if let Some(state) = storage_diff {
        for (slot, value) in state {
            acc.storage.insert(
                slot.into(),
                EvmStorageSlot {
                    // we use inverted value here to ensure that storage is treated as changed
                    original_value: (!value).into(),
                    present_value: value.into(),
                    is_cold: false,
                    transaction_id: 0,
                },
            );
        }
    }

    db.commit(HashMap::from_iter([(account, acc)]));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{address, bytes};
    use revm::database::EmptyDB;

    #[test]
    fn test_state_override_state() {
        let code = bytes!(
            "0x63d0e30db05f525f5f6004601c3473c02aaa39b223fe8d0a0e5c4f27ead9083c756cc25af15f5260205ff3"
        );
        let to = address!("0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599");

        let mut db = State::builder().with_database(CacheDB::new(EmptyDB::new())).build();

        let acc_override = AccountOverride::default().with_code(code.clone());
        apply_account_override(to, acc_override, &mut db).unwrap();

        let account = db.basic(to).unwrap().unwrap();
        assert!(account.code.is_some());
        assert_eq!(account.code_hash, keccak256(&code));
    }

    #[test]
    fn test_state_override_cache_db() {
        let code = bytes!(
            "0x63d0e30db05f525f5f6004601c3473c02aaa39b223fe8d0a0e5c4f27ead9083c756cc25af15f5260205ff3"
        );
        let to = address!("0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599");

        let mut db = CacheDB::new(EmptyDB::new());

        let acc_override = AccountOverride::default().with_code(code.clone());
        apply_account_override(to, acc_override, &mut db).unwrap();

        let account = db.basic(to).unwrap().unwrap();
        assert!(account.code.is_some());
        assert_eq!(account.code_hash, keccak256(&code));
    }

    #[test]
    fn test_state_override_storage() {
        let account = address!("0x1234567890123456789012345678901234567890");
        let slot1 = B256::from(U256::from(1));
        let slot2 = B256::from(U256::from(2));
        let value1 = B256::from(U256::from(100));
        let value2 = B256::from(U256::from(200));

        let mut db = CacheDB::new(EmptyDB::new());

        // Create storage overrides
        let mut storage = HashMap::<B256, B256>::default();
        storage.insert(slot1, value1);
        storage.insert(slot2, value2);

        let acc_override = AccountOverride::default().with_state_diff(storage);
        apply_account_override(account, acc_override, &mut db).unwrap();

        // Get the storage value using the database interface
        let storage1 = db.storage(account, U256::from(1)).unwrap();
        let storage2 = db.storage(account, U256::from(2)).unwrap();

        assert_eq!(storage1, U256::from(100));
        assert_eq!(storage2, U256::from(200));
    }

    #[test]
    fn test_state_override_full_state() {
        let account = address!("0x1234567890123456789012345678901234567890");
        let slot1 = B256::from(U256::from(1));
        let slot2 = B256::from(U256::from(2));
        let value1 = B256::from(U256::from(100));
        let value2 = B256::from(U256::from(200));

        let mut db = State::builder().with_database(CacheDB::new(EmptyDB::new())).build();

        // Create storage overrides using state (not state_diff)
        let mut storage = HashMap::<B256, B256>::default();
        storage.insert(slot1, value1);
        storage.insert(slot2, value2);

        let acc_override = AccountOverride::default().with_state(storage);
        let mut state_overrides = StateOverride::default();
        state_overrides.insert(account, acc_override);
        apply_state_overrides(state_overrides, &mut db).unwrap();

        // Get the storage value using the database interface
        let storage1 = db.storage(account, U256::from(1)).unwrap();
        let storage2 = db.storage(account, U256::from(2)).unwrap();

        assert_eq!(storage1, U256::from(100));
        assert_eq!(storage2, U256::from(200));
    }
}
