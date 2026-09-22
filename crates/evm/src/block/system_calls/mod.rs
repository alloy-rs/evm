//! System contract call functions.

use crate::{
    block::{BlockExecutionError, BlockValidationError},
    Evm, EvmError,
};
use alloy_consensus::BlockHeader;
use alloy_eips::{
    eip7002::WITHDRAWAL_REQUEST_TYPE, eip7251::CONSOLIDATION_REQUEST_TYPE, eip7685::Requests,
};
use alloy_hardforks::EthereumHardforks;
use alloy_primitives::{Bytes, B256};
use revm::{context::Block, DatabaseCommit};

mod eip2935;
mod eip4788;
mod eip7002;
mod eip7251;
mod eip8282;

pub use eip8282::{
    BUILDER_DEPOSIT_REQUEST_PREDEPLOY_ADDRESS, BUILDER_DEPOSIT_REQUEST_TYPE,
    BUILDER_EXIT_REQUEST_PREDEPLOY_ADDRESS, BUILDER_EXIT_REQUEST_TYPE,
};

/// An ephemeral helper type for executing system calls.
///
/// This can be used to chain system transaction calls.
#[derive(derive_more::Debug)]
pub struct SystemCaller<Spec> {
    spec: Spec,
}

impl<Spec> SystemCaller<Spec> {
    /// Create a new system caller with the given EVM config, database, and chain spec, and creates
    /// the EVM with the given initialized config and block environment.
    pub const fn new(spec: Spec) -> Self {
        Self { spec }
    }
}

impl<Spec> SystemCaller<Spec>
where
    Spec: EthereumHardforks,
{
    /// Apply pre execution changes.
    pub fn apply_pre_execution_changes(
        &mut self,
        header: impl BlockHeader,
        evm: &mut impl Evm<DB: DatabaseCommit>,
    ) -> Result<(), BlockExecutionError> {
        self.apply_blockhashes_contract_call(header.parent_hash(), evm)?;
        self.apply_beacon_root_contract_call(header.parent_beacon_block_root(), evm)?;

        Ok(())
    }

    /// Apply post execution changes.
    pub fn apply_post_execution_changes(
        &mut self,
        evm: &mut impl Evm<DB: DatabaseCommit>,
    ) -> Result<Requests, BlockExecutionError> {
        let mut requests = Requests::default();
        self.append_post_execution_changes(evm, &mut requests)?;
        Ok(requests)
    }

    /// Apply post execution changes, appending any requests to the provided container.
    pub fn append_post_execution_changes(
        &mut self,
        evm: &mut impl Evm<DB: DatabaseCommit>,
        requests: &mut Requests,
    ) -> Result<(), BlockExecutionError> {
        // Collect all EIP-7685 requests
        let withdrawal_requests = self.apply_withdrawal_requests_contract_call(evm)?;
        if !withdrawal_requests.is_empty() {
            requests.push_request_with_type(WITHDRAWAL_REQUEST_TYPE, withdrawal_requests);
        }

        // Collect all EIP-7251 requests
        let consolidation_requests = self.apply_consolidation_requests_contract_call(evm)?;
        if !consolidation_requests.is_empty() {
            requests.push_request_with_type(CONSOLIDATION_REQUEST_TYPE, consolidation_requests);
        }

        // Collect all EIP-8282 builder execution requests, introduced in Amsterdam. The system
        // calls must run from the Amsterdam activation block onward (they also reset each
        // predeploy's excess counter from the `EXCESS_INHIBITOR` sentinel to 0 on first call).
        if self.spec.is_amsterdam_active_at_timestamp(evm.block().timestamp().saturating_to()) {
            // EIP-8282 builder deposit requests
            let builder_deposit_requests =
                self.apply_builder_deposit_requests_contract_call(evm)?;
            if !builder_deposit_requests.is_empty() {
                requests
                    .push_request_with_type(BUILDER_DEPOSIT_REQUEST_TYPE, builder_deposit_requests);
            }

            // EIP-8282 builder exit requests
            let builder_exit_requests = self.apply_builder_exit_requests_contract_call(evm)?;
            if !builder_exit_requests.is_empty() {
                requests.push_request_with_type(BUILDER_EXIT_REQUEST_TYPE, builder_exit_requests);
            }
        }

        Ok(())
    }

    /// Applies the pre-block call to the EIP-2935 blockhashes contract.
    pub fn apply_blockhashes_contract_call(
        &mut self,
        parent_block_hash: B256,
        evm: &mut impl Evm<DB: DatabaseCommit>,
    ) -> Result<(), BlockExecutionError> {
        let _span = tracing::debug_span!("eip2935_blockhashes").entered();
        let result_and_state =
            eip2935::transact_blockhashes_contract_call(&self.spec, parent_block_hash, evm)?;

        if let Some(res) = result_and_state {
            evm.db_mut().commit(res.state);
        }

        Ok(())
    }

    /// Applies the pre-block call to the EIP-4788 beacon root contract.
    pub fn apply_beacon_root_contract_call(
        &mut self,
        parent_beacon_block_root: Option<B256>,
        evm: &mut impl Evm<DB: DatabaseCommit>,
    ) -> Result<(), BlockExecutionError> {
        let _span = tracing::debug_span!("eip4788_beacon_root").entered();
        let result_and_state =
            eip4788::transact_beacon_root_contract_call(&self.spec, parent_beacon_block_root, evm)?;

        if let Some(res) = result_and_state {
            evm.db_mut().commit(res.state);
        }

        Ok(())
    }

    /// Applies the post-block call to the EIP-7002 withdrawal requests contract.
    pub fn apply_withdrawal_requests_contract_call(
        &mut self,
        evm: &mut impl Evm<DB: DatabaseCommit>,
    ) -> Result<Bytes, BlockExecutionError> {
        let _span = tracing::debug_span!("eip7002_withdrawal_requests").entered();
        let result_and_state = eip7002::transact_withdrawal_requests_contract_call(evm)?;

        evm.db_mut().commit(result_and_state.state);

        eip7002::post_commit(result_and_state.result)
    }

    /// Applies the post-block call to the EIP-7251 consolidation requests contract.
    pub fn apply_consolidation_requests_contract_call(
        &mut self,
        evm: &mut impl Evm<DB: DatabaseCommit>,
    ) -> Result<Bytes, BlockExecutionError> {
        let _span = tracing::debug_span!("eip7251_consolidation_requests").entered();
        let result_and_state = eip7251::transact_consolidation_requests_contract_call(evm)?;

        evm.db_mut().commit(result_and_state.state);

        eip7251::post_commit(result_and_state.result)
    }

    /// Applies the post-block call to the EIP-8282 builder deposit requests contract.
    pub fn apply_builder_deposit_requests_contract_call(
        &mut self,
        evm: &mut impl Evm<DB: DatabaseCommit>,
    ) -> Result<Bytes, BlockExecutionError> {
        let _span = tracing::debug_span!("eip8282_builder_deposit_requests").entered();
        let result_and_state = eip8282::transact_builder_deposit_requests_contract_call(evm)?;

        evm.db_mut().commit(result_and_state.state);

        eip8282::deposit_post_commit(result_and_state.result)
    }

    /// Applies the post-block call to the EIP-8282 builder exit requests contract.
    pub fn apply_builder_exit_requests_contract_call(
        &mut self,
        evm: &mut impl Evm<DB: DatabaseCommit>,
    ) -> Result<Bytes, BlockExecutionError> {
        let _span = tracing::debug_span!("eip8282_builder_exit_requests").entered();
        let result_and_state = eip8282::transact_builder_exit_requests_contract_call(evm)?;

        evm.db_mut().commit(result_and_state.state);

        eip8282::exit_post_commit(result_and_state.result)
    }
}

// Fatal EVM errors describe execution infrastructure, not invalid blocks. Keep their
// source chain so callers can distinguish unavailable state from other failures.
fn system_call_error(context: BlockValidationError, source: impl EvmError) -> BlockExecutionError {
    if source.is_fatal() {
        BlockExecutionError::other(SystemCallError { context, source })
    } else {
        context.into()
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{context}")]
struct SystemCallError<E> {
    context: BlockValidationError,
    #[source]
    source: E,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{eth::EthEvmFactory, EvmEnv, EvmFactory};
    use alloc::string::ToString;
    use alloy_hardforks::{EthereumChainHardforks, EthereumHardfork, ForkCondition};
    use alloy_primitives::{Address, U256};
    use core::error::Error;
    use revm::{
        bytecode::Bytecode, database_interface::DBErrorMarker, state::AccountInfo, Database,
    };

    #[derive(Debug, thiserror::Error)]
    #[error("state unavailable")]
    struct StateError {
        fatal: bool,
    }

    impl DBErrorMarker for StateError {
        fn is_fatal(&self) -> bool {
            self.fatal
        }
    }

    #[derive(Debug)]
    struct UnavailableState(bool);

    impl Database for UnavailableState {
        type Error = StateError;

        fn basic(&mut self, _: Address) -> Result<Option<AccountInfo>, Self::Error> {
            Err(StateError { fatal: self.0 })
        }

        fn code_by_hash(&mut self, _: B256) -> Result<Bytecode, Self::Error> {
            Err(StateError { fatal: self.0 })
        }

        fn storage(&mut self, _: Address, _: U256) -> Result<U256, Self::Error> {
            Err(StateError { fatal: self.0 })
        }

        fn block_hash(&mut self, _: u64) -> Result<B256, Self::Error> {
            Err(StateError { fatal: self.0 })
        }
    }

    #[test_case::test_case(0, "failed to apply blockhash contract call")]
    #[test_case::test_case(1, "failed to apply beacon root contract call")]
    #[test_case::test_case(2, "failed to apply withdrawal requests contract call")]
    #[test_case::test_case(3, "failed to apply consolidation requests contract call")]
    #[test_case::test_case(4, "failed to apply builder deposit requests contract call")]
    #[test_case::test_case(5, "failed to apply builder exit requests contract call")]
    fn system_call_preserves_fatal_database_error(call: u8, context: &str) {
        for fatal in [true, false] {
            let spec = EthereumChainHardforks::new([
                (EthereumHardfork::Cancun, ForkCondition::Timestamp(0)),
                (EthereumHardfork::Prague, ForkCondition::Timestamp(0)),
            ]);
            let mut env: EvmEnv = EvmEnv::default();
            env.block_env.number = U256::ONE;
            let mut evm = EthEvmFactory.create_evm(UnavailableState(fatal), env);
            let error = match call {
                0 => eip2935::transact_blockhashes_contract_call(&spec, B256::ZERO, &mut evm)
                    .map(|_| ()),
                1 => eip4788::transact_beacon_root_contract_call(&spec, Some(B256::ZERO), &mut evm)
                    .map(|_| ()),
                2 => eip7002::transact_withdrawal_requests_contract_call(&mut evm).map(|_| ()),
                3 => eip7251::transact_consolidation_requests_contract_call(&mut evm).map(|_| ()),
                4 => eip8282::transact_builder_deposit_requests_contract_call(&mut evm).map(|_| ()),
                5 => eip8282::transact_builder_exit_requests_contract_call(&mut evm).map(|_| ()),
                _ => unreachable!(),
            }
            .unwrap_err();
            assert!(error.to_string().starts_with(context), "{error}");
            assert!(error.to_string().contains("state unavailable"), "{error}");
            if fatal {
                assert!(
                    error.as_internal().is_some(),
                    "database failures must not invalidate the block: {error:?}"
                );
                let mut source = error.source();
                let mut found = false;
                while let Some(error) = source {
                    if error.downcast_ref::<StateError>().is_some() {
                        found = true;
                        break;
                    }
                    source = error.source();
                }
                assert!(found, "database error type was lost: {error:?}");
            } else {
                assert!(
                    error.as_validation().is_some(),
                    "nonfatal errors retain validation classification"
                );
            }
        }
    }
}
