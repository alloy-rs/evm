//! System contract call functions.

use crate::{
    block::{BlockExecutionError, OnStateHook},
    Evm,
};
use alloc::{borrow::Cow, boxed::Box};
use alloy_consensus::BlockHeader;
use alloy_eips::{
    eip7002::WITHDRAWAL_REQUEST_TYPE, eip7251::CONSOLIDATION_REQUEST_TYPE, eip7685::Requests,
};
use alloy_hardforks::EthereumHardforks;
use alloy_primitives::{Bytes, B256};
use revm::{state::EvmState, DatabaseCommit};

use super::{StateChangePostBlockSource, StateChangePreBlockSource, StateChangeSource};

mod eip2935;
mod eip4788;
mod eip7002;
mod eip7251;

/// An ephemeral helper type for executing system calls.
///
/// This can be used to chain system transaction calls.
#[derive(derive_more::Debug)]
pub struct SystemCaller<Spec> {
    spec: Spec,
    /// Optional hook to be called after each state change.
    #[debug(skip)]
    hook: Option<Box<dyn OnStateHook>>,
}

impl<Spec> SystemCaller<Spec> {
    /// Create a new system caller with the given EVM config, database, and chain spec, and creates
    /// the EVM with the given initialized config and block environment.
    pub const fn new(spec: Spec) -> Self {
        Self { spec, hook: None }
    }

    /// Installs a custom hook to be called after each state change.
    pub fn with_state_hook(&mut self, hook: Option<Box<dyn OnStateHook>>) -> &mut Self {
        self.hook = hook;
        self
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
            if let Some(hook) = &mut self.hook {
                hook.on_state(
                    StateChangeSource::PreBlock(StateChangePreBlockSource::BlockHashesContract),
                    &res.state,
                );
            }
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
            if let Some(hook) = &mut self.hook {
                hook.on_state(
                    StateChangeSource::PreBlock(StateChangePreBlockSource::BeaconRootContract),
                    &res.state,
                );
            }
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

        if let Some(ref mut hook) = &mut self.hook {
            hook.on_state(
                StateChangeSource::PostBlock(
                    StateChangePostBlockSource::WithdrawalRequestsContract,
                ),
                &result_and_state.state,
            );
        }
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

        if let Some(ref mut hook) = &mut self.hook {
            hook.on_state(
                StateChangeSource::PostBlock(
                    StateChangePostBlockSource::ConsolidationRequestsContract,
                ),
                &result_and_state.state,
            );
        }
        evm.db_mut().commit(result_and_state.state);

        eip7251::post_commit(result_and_state.result)
    }

    /// Delegate to stored `OnStateHook`, noop if hook is `None`.
    pub fn on_state(&mut self, source: StateChangeSource, state: &EvmState) {
        if let Some(hook) = &mut self.hook {
            hook.on_state(source, state);
        }
    }

    /// Invokes the state hook with the outcome of the given closure, forwards error if any.
    pub fn try_on_state_with<'a, F, E>(&mut self, f: F) -> Result<(), E>
    where
        F: FnOnce() -> Result<(StateChangeSource, Cow<'a, EvmState>), E>,
    {
        self.invoke_hook_with(|hook| {
            let (source, state) = f()?;
            hook.on_state(source, &state);
            Ok(())
        })
        .unwrap_or(Ok(()))
    }

    /// Invokes the state hook with the outcome of the given closure.
    pub fn on_state_with<'a, F>(&mut self, f: F)
    where
        F: FnOnce() -> (StateChangeSource, Cow<'a, EvmState>),
    {
        self.invoke_hook_with(|hook| {
            let (source, state) = f();
            hook.on_state(source, &state);
        });
    }

    /// Invokes the given closure with the configured state hook if any.
    pub fn invoke_hook_with<F, R>(&mut self, f: F) -> Option<R>
    where
        F: FnOnce(&mut Box<dyn OnStateHook>) -> R,
    {
        self.hook.as_mut().map(f)
    }
}
