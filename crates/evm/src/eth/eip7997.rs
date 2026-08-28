//! [EIP-7997](https://eips.ethereum.org/EIPS/eip-7997) deterministic factory support.

use crate::{block::BlockExecutionError, Database};
use alloy_primitives::{address, bytes, Address, Bytes};
use revm::{
    state::{Account, Bytecode, EvmState},
    DatabaseCommit,
};

/// Address of the EIP-7997 deterministic factory.
pub const DETERMINISTIC_FACTORY_ADDRESS: Address =
    address!("0x4e59b44847b379578588920cA78FbF26c0B4956C");

/// Returns the EIP-7997 deterministic factory runtime code.
pub const fn deterministic_factory_runtime_code() -> Bytes {
    bytes!(
        "0x7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe03601600081602082378035828234f58015156039578182fd5b8082525050506014600cf3"
    )
}

/// Returns the EIP-7997 deterministic factory runtime bytecode.
pub fn deterministic_factory_runtime_bytecode() -> Bytecode {
    Bytecode::new_legacy(deterministic_factory_runtime_code())
}

/// Inserts the EIP-7997 deterministic factory account if the current state does not already match.
///
/// If the account nonce is zero, it is set to one. Any existing nonzero nonce is preserved.
pub fn insert_deterministic_factory_account<DB>(db: &mut DB) -> Result<(), BlockExecutionError>
where
    DB: Database + DatabaseCommit,
{
    let code = deterministic_factory_runtime_bytecode();
    let code_hash = code.hash_slow();
    let info = db
        .basic(DETERMINISTIC_FACTORY_ADDRESS)
        .map_err(BlockExecutionError::other)?
        .unwrap_or_default();

    if info.nonce != 0 && info.code_hash == code_hash {
        return Ok(());
    }

    let mut account = Account::from(info);
    if account.info.nonce == 0 {
        // Mirror normal contract creation semantics: deployed contracts start with nonce 1.
        account.info.nonce = 1;
    }
    account.info.code_hash = code_hash;
    account.info.code = Some(code);
    account.mark_touch();

    db.commit(EvmState::from_iter([(DETERMINISTIC_FACTORY_ADDRESS, account)]));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use revm::{
        database::{CacheDB, State},
        database_interface::EmptyDB,
        state::AccountInfo,
        Database as _, DatabaseCommit,
    };

    #[test]
    fn inserts_factory_account() {
        let mut db = State::builder().with_database(CacheDB::new(EmptyDB::new())).build();

        insert_deterministic_factory_account(&mut db).unwrap();

        let info = db.basic(DETERMINISTIC_FACTORY_ADDRESS).unwrap().unwrap();
        let code = info.code.unwrap();
        assert_eq!(info.nonce, 1);
        assert_eq!(info.code_hash, deterministic_factory_runtime_bytecode().hash_slow());
        assert_eq!(code.original_bytes(), deterministic_factory_runtime_code());
    }

    #[test]
    fn preserves_existing_nonzero_nonce() {
        let mut db = State::builder().with_database(CacheDB::new(EmptyDB::new())).build();
        let mut account = Account::from(AccountInfo { nonce: 7, ..Default::default() });
        account.mark_touch();
        db.commit(EvmState::from_iter([(DETERMINISTIC_FACTORY_ADDRESS, account)]));

        insert_deterministic_factory_account(&mut db).unwrap();

        let info = db.basic(DETERMINISTIC_FACTORY_ADDRESS).unwrap().unwrap();
        assert_eq!(info.nonce, 7);
        assert_eq!(info.code_hash, deterministic_factory_runtime_bytecode().hash_slow());
    }
}
