//! [EIP-7997](https://eips.ethereum.org/EIPS/eip-7997) deterministic factory predeploy.
//!
//! EIP-7997 inserts a minimal `CREATE2` factory as a contract at a fixed address upon activation
//! of the Amsterdam hardfork, enabling deterministic deployments at identical addresses across
//! EVM chains.

use crate::{block::BlockExecutionError, Evm};
use alloy_hardforks::EthereumHardforks;
use alloy_primitives::{address, b256, bytes, Address, Bytes, B256};
use revm::{
    context::Block,
    state::{Account, AccountStatus, Bytecode, EvmState},
    Database,
};

/// Address of the EIP-7997 deterministic `CREATE2` factory predeploy.
pub const FACTORY_ADDRESS: Address = address!("0x0000000000000000000000000000000000000012");

/// Runtime bytecode deployed at [`FACTORY_ADDRESS`] on activation of EIP-7997.
///
/// When called, it invokes `CREATE2` using the first 32 bytes of the input as the salt and the
/// remaining bytes as the init code, forwarding the call value.
pub const FACTORY_CODE: Bytes = bytes!(
    "0x60203610602f5760003560203603806020600037600034f5806026573d600060003e3d6000fd5b60005260206000f35b60006000fd"
);

/// keccak256 hash of [`FACTORY_CODE`].
///
/// Used to detect whether the predeploy is already present, keeping the insertion idempotent.
pub const FACTORY_CODE_HASH: B256 =
    b256!("0x2b2a8b8ac51cd14e2371a09cc5fa3737e74cabd8dafad0573ebca41c7cd5b51f");

/// Builds the pre-block state change that inserts the EIP-7997 factory bytecode at
/// [`FACTORY_ADDRESS`].
///
/// Returns `None` (a no-op) when Amsterdam is not active, or when the factory code is already
/// present at the address. Comparing the existing code hash makes the insertion idempotent, so it
/// only mutates state once: on the block that activates Amsterdam.
///
/// The existing balance is preserved and the nonce is set to `1`, matching the account state of a
/// deployed contract.
///
/// Note: this does not commit the state change to the database, it only constructs it.
#[inline]
pub(crate) fn build_factory_predeploy_state(
    spec: impl EthereumHardforks,
    evm: &mut impl Evm,
) -> Result<Option<EvmState>, BlockExecutionError> {
    if !spec.is_amsterdam_active_at_timestamp(evm.block().timestamp().saturating_to()) {
        return Ok(None);
    }

    let mut info = evm
        .db_mut()
        .basic(FACTORY_ADDRESS)
        .map_err(|_| BlockExecutionError::msg("could not load EIP-7997 factory account"))?
        .unwrap_or_default();

    // Idempotent: only insert the factory on the block that activates Amsterdam. On later blocks
    // the code is already present, so this is a no-op.
    if info.code_hash == FACTORY_CODE_HASH {
        return Ok(None);
    }

    info.nonce = 1;
    info.code_hash = FACTORY_CODE_HASH;
    info.code = Some(Bytecode::new_legacy(FACTORY_CODE));

    let mut account = Account::from(info);
    account.status = AccountStatus::Touched;

    Ok(Some(EvmState::from_iter([(FACTORY_ADDRESS, account)])))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EthEvmFactory, EvmEnv, EvmFactory};
    use alloy_hardforks::{EthereumHardfork, ForkCondition};
    use alloy_primitives::{keccak256, U256};
    use revm::{
        context::{BlockEnv, CfgEnv},
        database::{CacheDB, EmptyDB, State},
        primitives::hardfork::SpecId,
        state::AccountInfo,
    };

    /// Minimal spec that reports Amsterdam as active from genesis when `amsterdam` is set.
    struct TestSpec {
        amsterdam: bool,
    }

    impl EthereumHardforks for TestSpec {
        fn ethereum_fork_activation(&self, fork: EthereumHardfork) -> ForkCondition {
            if self.amsterdam && fork == EthereumHardfork::Amsterdam {
                ForkCondition::Timestamp(0)
            } else {
                ForkCondition::Never
            }
        }
    }

    fn evm_with(db: CacheDB<EmptyDB>) -> impl Evm {
        let mut cfg = CfgEnv::default();
        cfg.spec = SpecId::AMSTERDAM;
        let env = EvmEnv {
            block_env: BlockEnv { timestamp: U256::from(1), ..Default::default() },
            cfg_env: cfg,
        };
        let db = State::builder().with_database(db).with_bundle_update().build();
        EthEvmFactory.create_evm(db, env)
    }

    #[test]
    fn factory_code_hash_matches_code() {
        assert_eq!(keccak256(&FACTORY_CODE), FACTORY_CODE_HASH);
        assert_eq!(Bytecode::new_legacy(FACTORY_CODE).hash_slow(), FACTORY_CODE_HASH);
    }

    #[test]
    fn inserts_factory_when_amsterdam_active() {
        let mut evm = evm_with(CacheDB::new(EmptyDB::new()));

        let state = build_factory_predeploy_state(&TestSpec { amsterdam: true }, &mut evm)
            .unwrap()
            .expect("factory predeploy state");

        let account = state.get(&FACTORY_ADDRESS).expect("factory account");
        assert_eq!(account.info.code_hash, FACTORY_CODE_HASH);
        assert_eq!(account.info.code.as_ref().unwrap().original_bytes(), FACTORY_CODE);
        assert_eq!(account.info.nonce, 1);
        assert!(account.is_touched());
        assert_eq!(state.len(), 1);
    }

    #[test]
    fn idempotent_when_already_deployed() {
        let mut db = CacheDB::new(EmptyDB::new());
        // Pretend the factory is already deployed at the target address.
        db.insert_account_info(
            FACTORY_ADDRESS,
            AccountInfo {
                nonce: 1,
                code_hash: FACTORY_CODE_HASH,
                code: Some(Bytecode::new_legacy(FACTORY_CODE)),
                ..Default::default()
            },
        );
        let mut evm = evm_with(db);

        assert!(build_factory_predeploy_state(&TestSpec { amsterdam: true }, &mut evm)
            .unwrap()
            .is_none());
    }

    #[test]
    fn noop_before_amsterdam() {
        let mut evm = evm_with(CacheDB::new(EmptyDB::new()));

        assert!(build_factory_predeploy_state(&TestSpec { amsterdam: false }, &mut evm)
            .unwrap()
            .is_none());
    }
}
