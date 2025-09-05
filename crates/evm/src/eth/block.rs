//! Ethereum block executor.

use alloc::collections::BTreeMap;

use super::{
    dao_fork, eip6110,
    receipt_builder::{AlloyReceiptBuilder, ReceiptBuilder, ReceiptBuilderCtx},
    spec::{EthExecutorSpec, EthSpec},
    EthEvmFactory,
};
use crate::{
    block::{
        state_changes::{balance_increment_state, post_block_balance_increments},
        BlockExecutionError, BlockExecutionResult, BlockExecutor, BlockExecutorFactory,
        BlockExecutorFor, BlockValidationError, CommitChanges, ExecutableTx, OnStateHook,
        StateChangePostBlockSource, StateChangeSource, SystemCaller,
    },
    Database, Evm, EvmFactory, FromRecoveredTx, FromTxWithEncoded,
};
use alloc::{borrow::Cow, boxed::Box, vec::Vec};
use alloy_block_access_list::{
    balance_change::BalanceChange, code_change::CodeChange, nonce_change::NonceChange,
    AccountChanges, BlockAccessIndex, BlockAccessList, SlotChanges, StorageChange, MAX_CODE_SIZE,
    MAX_TXS,
};
use alloy_consensus::{Header, Transaction, TxReceipt};
use alloy_eips::{
    eip4895::Withdrawals, eip7002::WITHDRAWAL_REQUEST_PREDEPLOY_ADDRESS,
    eip7251::CONSOLIDATION_REQUEST_PREDEPLOY_ADDRESS, eip7685::Requests, Encodable2718,
};
use alloy_hardforks::EthereumHardfork;
use alloy_primitives::{Address, Log, B256, U256};
use revm::{
    context::result::ExecutionResult,
    context_interface::result::ResultAndState,
    database::State,
    primitives::{StorageKey, StorageValue},
    state::AccountInfo,
    DatabaseCommit, Inspector,
};

/// Context for Ethereum block execution.
#[derive(Debug, Clone)]
pub struct EthBlockExecutionCtx<'a> {
    /// Parent block hash.
    pub parent_hash: B256,
    /// Parent beacon block root.
    pub parent_beacon_block_root: Option<B256>,
    /// Block ommers
    pub ommers: &'a [Header],
    /// Block withdrawals.
    pub withdrawals: Option<Cow<'a, Withdrawals>>,
}

/// Block executor for Ethereum.
#[derive(Debug)]
pub struct EthBlockExecutor<'a, Evm, Spec, R: ReceiptBuilder> {
    /// Reference to the specification object.
    spec: Spec,

    /// Context for block execution.
    pub ctx: EthBlockExecutionCtx<'a>,
    /// Inner EVM.
    evm: Evm,
    /// Utility to call system smart contracts.
    system_caller: SystemCaller<Spec>,
    /// Receipt builder.
    receipt_builder: R,

    /// Receipts of executed transactions.
    receipts: Vec<R::Receipt>,
    /// Total gas used by transactions in this block.
    gas_used: u64,
    /// Optional store for building bal
    pub block_access_list: Option<BlockAccessList>,
    /// All touched accounts in the block.
    pub touched_addresses: Vec<Address>,
}

impl<'a, Evm, Spec, R> EthBlockExecutor<'a, Evm, Spec, R>
where
    Spec: Clone,
    R: ReceiptBuilder,
{
    /// Creates a new [`EthBlockExecutor`]
    pub fn new(evm: Evm, ctx: EthBlockExecutionCtx<'a>, spec: Spec, receipt_builder: R) -> Self {
        Self {
            evm,
            ctx,
            receipts: Vec::new(),
            gas_used: 0,
            system_caller: SystemCaller::new(spec.clone()),
            spec,
            receipt_builder,
            block_access_list: Some(BlockAccessList::default()),
            touched_addresses: Vec::new(),
        }
    }
}

impl<'db, DB, E, Spec, R> BlockExecutor for EthBlockExecutor<'_, E, Spec, R>
where
    DB: Database + 'db,
    E: Evm<
        DB = &'db mut State<DB>,
        Tx: FromRecoveredTx<R::Transaction> + FromTxWithEncoded<R::Transaction>,
    >,
    Spec: EthExecutorSpec,
    R: ReceiptBuilder<Transaction: Transaction + Encodable2718, Receipt: TxReceipt<Log = Log>>,
{
    type Transaction = R::Transaction;
    type Receipt = R::Receipt;
    type Evm = E;

    fn apply_pre_execution_changes(&mut self) -> Result<(), BlockExecutionError> {
        // Set state clear flag if the block is after the Spurious Dragon hardfork.
        let state_clear_flag =
            self.spec.is_spurious_dragon_active_at_block(self.evm.block().number.saturating_to());
        self.evm.db_mut().set_state_clear_flag(state_clear_flag);

        let contract_acc_change = self.system_caller.apply_blockhashes_contract_call(
            self.ctx.parent_hash,
            self.evm.block().number.saturating_to(),
            &mut self.evm,
        )?;

        self.block_access_list.clone().unwrap().push(contract_acc_change);

        self.system_caller.apply_beacon_root_contract_call(
            self.evm.block().timestamp.saturating_to(),
            self.ctx.parent_beacon_block_root,
            &mut self.evm,
        )?;

        Ok(())
    }

    fn execute_transaction_with_commit_condition(
        &mut self,
        tx: impl ExecutableTx<Self>,
        f: impl FnOnce(&ExecutionResult<<Self::Evm as Evm>::HaltReason>) -> CommitChanges,
    ) -> Result<Option<u64>, BlockExecutionError> {
        // The sum of the transaction's gas limit, Tg, and the gas utilized in this block prior,
        // must be no greater than the block's gasLimit.
        let block_available_gas = self.evm.block().gas_limit - self.gas_used;

        if tx.tx().gas_limit() > block_available_gas {
            return Err(BlockValidationError::TransactionGasLimitMoreThanAvailableBlockGas {
                transaction_gas_limit: tx.tx().gas_limit(),
                block_available_gas,
            }
            .into());
        }

        // Execute transaction.
        let ResultAndState { result, mut state } = self
            .evm
            .transact(&tx)
            .map_err(|err| BlockExecutionError::evm(err, tx.tx().trie_hash()))?;

        if !f(&result).should_commit() {
            return Ok(None);
        }
        // updated
        self.system_caller.on_state(StateChangeSource::Transaction(self.receipts.len()), &state);

        let gas_used = result.gas_used();

        // append gas used
        self.gas_used += gas_used;

        // Push transaction changeset and calculate header bloom filter for receipt.
        self.receipts.push(self.receipt_builder.build_receipt(ReceiptBuilderCtx {
            tx: tx.tx(),
            evm: &self.evm,
            result: result.clone(),
            state: &state,
            cumulative_gas_used: self.gas_used,
        }));

        // Commit the state changes.
        self.evm.db_mut().commit(state.clone());
        match tx.tx().kind() {
            alloy_primitives::TxKind::Create => {
                if let Some(created_address) = result.created_address() {
                    if let Some(acc) = state.get(&created_address) {
                        if let Some(bal) = self.block_access_list.as_mut() {
                            bal.push(from_account_with_tx_index(
                                created_address,
                                self.receipts.len() as u64,
                                &acc.info,
                            ));
                            tracing::debug!(
                                "BlockAccessList: new contract created at {:#x}, tx_index={}, storage: {:#?}",
                                created_address,
                                self.receipts.len(),
                                acc.info.storage_access,
                            );
                            state.get_mut(&created_address).unwrap().info.clear_state_changes();
                        }
                    }
                }
            }
            alloy_primitives::TxKind::Call(address) => {
                if let Some(acc) = state.get(&address) {
                    if let Some(bal) = self.block_access_list.as_mut() {
                        bal.push(from_account_with_tx_index(
                            address,
                            self.receipts.len() as u64,
                            &acc.info,
                        ));
                        tracing::debug!(
                            "BlockAccessList: Tx Call arm {:#x}, tx_index={}, storage: {:#?}",
                            address,
                            self.receipts.len(),
                            acc.info.storage_access,
                        );
                        state.get_mut(&address).unwrap().info.clear_state_changes();
                    }
                }
            }
        }
        if let Some(acc) = state.get(tx.signer()) {
            if let Some(bal) = self.block_access_list.as_mut() {
                bal.push(from_account_with_tx_index(
                    *tx.signer(),
                    self.receipts.len() as u64,
                    &acc.info,
                ));
                tracing::debug!(
                    "BlockAccessList: Tx signer arm tx_index={}, storage: {:#?}",
                    self.receipts.len(),
                    acc.info.storage_access,
                );
                state.get_mut(tx.signer()).unwrap().info.clear_state_changes();
            }
        }

        // Store access list changes in bal.
        if let Some(access_list) = tx.tx().access_list() {
            for item in &access_list.0 {
                let addr = item.address;
                if state.contains_key(&addr) {
                    if let Some(bal) = self.block_access_list.as_mut() {
                        bal.push(from_account_with_tx_index(
                            addr,
                            self.receipts.len() as u64,
                            &state.get(&addr).unwrap().info,
                        ));
                        state.get_mut(&addr).unwrap().info.clear_state_changes();
                    }
                }
            }
        }

        // if let Some(bal) = self.block_access_list.as_mut() {
        //     // collect addresses of all accounts we care about
        //     let addrs: Vec<_> = state
        //         .iter()
        //         .filter_map(|(addr, acc)| {
        //             let info = &acc.info;
        //             if !info.storage_access.reads.is_empty()
        //                 || !info.storage_access.writes.is_empty()
        //                 || !info.code_change.is_empty()
        //             {
        //                 Some(*addr)
        //             } else {
        //                 None
        //             }
        //         })
        //         .collect();

        //     // now safe to mutate
        //     for addr in addrs {
        //         if let Some(acc) = state.get(&addr) {
        //             bal.push(from_account_with_tx_index(
        //                 addr,
        //                 self.receipts.len() as u64,
        //                 &acc.info,
        //             ));
        //         }
        //         state.get_mut(&addr).unwrap().info.clear_state_changes();
        //     }
        // }

        // Commit the state changes.
        self.evm.db_mut().commit(state.clone());
        Ok(Some(gas_used))
    }

    fn finish(
        mut self,
    ) -> Result<(Self::Evm, BlockExecutionResult<R::Receipt>), BlockExecutionError> {
        let post_system_tx = self.receipts.len() + 1;
        let mut post_system_acc_changes: Vec<AccountChanges> = Vec::new();

        let requests = if self
            .spec
            .is_prague_active_at_timestamp(self.evm.block().timestamp.saturating_to())
        {
            // Collect all EIP-6110 deposits
            let deposit_requests =
                eip6110::parse_deposits_from_receipts(&self.spec, &self.receipts)?;

            let mut requests = Requests::default();

            if !deposit_requests.is_empty() {
                requests.push_request_with_type(eip6110::DEPOSIT_REQUEST_TYPE, deposit_requests);
            }

            let mut pre_withdrawal = Vec::new();
            for i in 0..=3 {
                let value = self
                    .evm
                    .db_mut()
                    .database
                    .storage(WITHDRAWAL_REQUEST_PREDEPLOY_ADDRESS, StorageKey::from(i))
                    .unwrap();
                pre_withdrawal.push(value);
            }

            let mut pre_consolidation = Vec::new();
            for i in 0..=3 {
                let value = self
                    .evm
                    .db_mut()
                    .database
                    .storage(CONSOLIDATION_REQUEST_PREDEPLOY_ADDRESS, StorageKey::from(i))
                    .unwrap();
                pre_consolidation.push(value);
            }

            requests.extend(self.system_caller.apply_post_execution_changes(&mut self.evm)?);

            let mut post_withdrawal = Vec::new();
            for i in 0..=3 {
                let value = self
                    .evm
                    .db_mut()
                    .database
                    .storage(WITHDRAWAL_REQUEST_PREDEPLOY_ADDRESS, StorageKey::from(i))
                    .unwrap();
                post_withdrawal.push(value);
            }

            let mut post_consolidation = Vec::new();
            for i in 0..=3 {
                let value = self
                    .evm
                    .db_mut()
                    .database
                    .storage(CONSOLIDATION_REQUEST_PREDEPLOY_ADDRESS, StorageKey::from(i))
                    .unwrap();
                post_consolidation.push(value);
            }

            post_system_acc_changes.push(build_post_execution_system_contract_account_change(
                WITHDRAWAL_REQUEST_PREDEPLOY_ADDRESS,
                pre_withdrawal,
                post_withdrawal,
                post_system_tx as BlockAccessIndex,
            ));
            post_system_acc_changes.push(build_post_execution_system_contract_account_change(
                CONSOLIDATION_REQUEST_PREDEPLOY_ADDRESS,
                pre_consolidation,
                post_consolidation,
                post_system_tx as BlockAccessIndex,
            ));

            requests
        } else {
            Requests::default()
        };

        let mut balance_increments = post_block_balance_increments(
            &self.spec,
            self.evm.block(),
            self.ctx.ommers,
            self.ctx.withdrawals.as_deref(),
        );

        // Irregular state change at Ethereum DAO hardfork
        if self
            .spec
            .ethereum_fork_activation(EthereumHardfork::Dao)
            .transitions_at_block(self.evm.block().number.saturating_to())
        {
            // drain balances from hardcoded addresses.
            let drained_balance: u128 = self
                .evm
                .db_mut()
                .drain_balances(dao_fork::DAO_HARDFORK_ACCOUNTS)
                .map_err(|_| BlockValidationError::IncrementBalanceFailed)?
                .into_iter()
                .sum();

            // return balance to DAO beneficiary.
            *balance_increments.entry(dao_fork::DAO_HARDFORK_BENEFICIARY).or_default() +=
                drained_balance;
        }
        // increment balances
        self.evm
            .db_mut()
            .increment_balances(balance_increments.clone())
            .map_err(|_| BlockValidationError::IncrementBalanceFailed)?;

        // call state hook with changes due to balance increments.
        self.system_caller.try_on_state_with(|| {
            balance_increment_state(&balance_increments, self.evm.db_mut()).map(|state| {
                (
                    StateChangeSource::PostBlock(StateChangePostBlockSource::BalanceIncrements),
                    Cow::Owned(state),
                )
            })
        })?;

        // Build Block-level Access List here
        // 1. Build for all pre execution (Done in `apply_pre_execution_changes`)
        // 2. Build for all the touched addresses.
        // 3. Build for all post execution
        // 4. Sort
        // for address in self.touched_addresses.iter() {
        //     if let Ok(Some(account)) = self.evm.db_mut().database.basic(*address) {
        //         let acc_change = from_account(*address, &account);
        //         self.block_access_list.as_mut().unwrap().account_changes.push(acc_change);
        //     }
        // }

        // All post tx balance increments
        for (address, increment) in balance_increments {
            if increment != 0 {
                self.block_access_list.as_mut().unwrap().push(
                    AccountChanges::default().with_address(address).with_balance_change(
                        BalanceChange {
                            block_access_index: post_system_tx as u64,
                            post_balance: U256::from(increment),
                        },
                    ),
                );
            }
        }

        // Add post execution system contract account changes
        self.block_access_list.as_mut().unwrap().extend(post_system_acc_changes);
        // self.block_access_list =
        //     Some(sort_and_remove_duplicates_in_bal(self.block_access_list.unwrap()));
        Ok((
            self.evm,
            BlockExecutionResult {
                receipts: self.receipts,
                requests,
                gas_used: self.gas_used,
                block_access_list: Some(sort_and_remove_duplicates_in_bal(
                    self.block_access_list.unwrap(),
                )),
            },
        ))
    }

    fn set_state_hook(&mut self, hook: Option<Box<dyn OnStateHook>>) {
        self.system_caller.with_state_hook(hook);
    }

    fn evm_mut(&mut self) -> &mut Self::Evm {
        &mut self.evm
    }

    fn evm(&self) -> &Self::Evm {
        &self.evm
    }
}

/// Ethereum block executor factory.
#[derive(Debug, Clone, Default, Copy)]
pub struct EthBlockExecutorFactory<
    R = AlloyReceiptBuilder,
    Spec = EthSpec,
    EvmFactory = EthEvmFactory,
> {
    /// Receipt builder.
    receipt_builder: R,
    /// Chain specification.
    spec: Spec,
    /// EVM factory.
    evm_factory: EvmFactory,
}

impl<R, Spec, EvmFactory> EthBlockExecutorFactory<R, Spec, EvmFactory> {
    /// Creates a new [`EthBlockExecutorFactory`] with the given spec, [`EvmFactory`], and
    /// [`ReceiptBuilder`].
    pub const fn new(receipt_builder: R, spec: Spec, evm_factory: EvmFactory) -> Self {
        Self { receipt_builder, spec, evm_factory }
    }

    /// Exposes the receipt builder.
    pub const fn receipt_builder(&self) -> &R {
        &self.receipt_builder
    }

    /// Exposes the chain specification.
    pub const fn spec(&self) -> &Spec {
        &self.spec
    }

    /// Exposes the EVM factory.
    pub const fn evm_factory(&self) -> &EvmFactory {
        &self.evm_factory
    }
}

impl<R, Spec, EvmF> BlockExecutorFactory for EthBlockExecutorFactory<R, Spec, EvmF>
where
    R: ReceiptBuilder<Transaction: Transaction + Encodable2718, Receipt: TxReceipt<Log = Log>>,
    Spec: EthExecutorSpec,
    EvmF: EvmFactory<Tx: FromRecoveredTx<R::Transaction> + FromTxWithEncoded<R::Transaction>>,
    Self: 'static,
{
    type EvmFactory = EvmF;
    type ExecutionCtx<'a> = EthBlockExecutionCtx<'a>;
    type Transaction = R::Transaction;
    type Receipt = R::Receipt;

    fn evm_factory(&self) -> &Self::EvmFactory {
        &self.evm_factory
    }

    fn create_executor<'a, DB, I>(
        &'a self,
        evm: EvmF::Evm<&'a mut State<DB>, I>,
        ctx: Self::ExecutionCtx<'a>,
    ) -> impl BlockExecutorFor<'a, Self, DB, I>
    where
        DB: Database + 'a,
        I: Inspector<EvmF::Context<&'a mut State<DB>>> + 'a,
    {
        EthBlockExecutor::new(evm, ctx, &self.spec, &self.receipt_builder)
    }
}

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
            let change = StorageChange { block_access_index: tx_index, new_value: post_val };
            account_changes
                .storage_changes
                .push(SlotChanges::default().with_slot(slot).with_change(change));
        } else {
            account_changes.storage_reads.push(slot.into());
        }
    }

    account_changes
}

/// An utility function to build block access list with tx_index
pub fn from_account_with_tx_index(
    address: Address,
    block_access_index: u64,
    account: &AccountInfo,
) -> AccountChanges {
    let mut account_changes = AccountChanges::default();

    for key in &account.storage_access.reads {
        account_changes.storage_reads.push((*key).into());
    }

    // Group writes by slots
    let mut slot_map: BTreeMap<StorageKey, Vec<StorageChange>> = BTreeMap::new();

    for (slot, (_pre, post)) in &account.storage_access.writes {
        slot_map
            .entry(*slot)
            .or_default()
            .push(StorageChange { block_access_index, new_value: *post });
    }

    // Convert slot_map into SlotChanges and push into account_changes
    for (slot, changes) in slot_map {
        account_changes.storage_changes.push(SlotChanges { slot, changes });
    }

    // False if zero value transfer
    if !account.balance_change.1 {
        account_changes
            .balance_changes
            .push(BalanceChange { block_access_index, post_balance: account.balance_change.0 });
    }

    let (pre_nonce, post_nonce) = account.nonce_change;
    if pre_nonce != post_nonce {
        account_changes
            .nonce_changes
            .push(NonceChange { block_access_index, new_nonce: post_nonce });
    }

    let code = &account.code_change;
    if !code.is_empty() {
        account_changes
            .code_changes
            .push(CodeChange { block_access_index, new_code: code.clone() });
    }

    account_changes.address = address;
    account_changes
}

/// Sort block-level access list and removes duplicates entries by merging them together.
pub fn sort_and_remove_duplicates_in_bal(mut bal: BlockAccessList) -> BlockAccessList {
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
    let max_block_access_index = MAX_TXS + 1; // 0 for pre-exec, 1..MAX_TXS for txs, MAX_TXS+1 for post-exec
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
    use alloy_consensus::{
        transaction::Recovered, EthereumTxEnvelope, SignableTransaction, TxLegacy,
    };
    use alloy_eips::{eip2718::WithEncoded, Encodable2718};
    use alloy_primitives::{address, Signature, TxKind, B256, U256};
    use revm::{
        database::{CacheDB, EmptyDB, State},
        state::AccountInfo,
    };

    use crate::{
        block::{BlockExecutor, BlockExecutorFactory},
        eth::{
            receipt_builder::AlloyReceiptBuilder, spec::EthSpec, EthBlockExecutionCtx,
            EthBlockExecutorFactory,
        },
        EthEvmFactory, EvmEnv, EvmFactory,
    };

    #[test]
    fn test_bal_building() {
        let ctx = EthBlockExecutionCtx {
            parent_hash: B256::ZERO,
            parent_beacon_block_root: Some(B256::ZERO),
            ommers: &[],
            withdrawals: None,
        };
        let legacy1 = TxLegacy {
            chain_id: None,
            nonce: 0,
            gas_price: 1,
            gas_limit: 21_000,
            to: TxKind::Call(address!("000000000000000000000000000000000000dead")),
            value: U256::from(100_000_000_000_u64),
            input: Default::default(),
        };
        let legacy2 = TxLegacy {
            chain_id: None,
            nonce: 1,
            gas_price: 1,
            gas_limit: 21_000,
            to: TxKind::Call(address!("000000000000000000000000000000000000dead")),
            value: U256::from(100_000_000_000_u64),
            input: Default::default(),
        };
        let sender = address!("000000000000000000000000000000000000beef");

        let factory = EthBlockExecutorFactory::new(
            AlloyReceiptBuilder::default(),
            EthSpec::mainnet(),
            EthEvmFactory::default(),
        );
        let mut db = State::builder().with_database(CacheDB::<EmptyDB>::default()).build();
        db.database.insert_account_info(
            sender,
            AccountInfo {
                balance: U256::from(50_000_000_000_000_000_u64),
                nonce: 0,
                code_hash: B256::ZERO,
                ..Default::default()
            },
        );
        db.database.insert_account_info(
            address!("000000000000000000000000000000000000dead"),
            AccountInfo {
                balance: U256::from(150_000_000_000_000_000_u64),
                nonce: 0,
                code_hash: B256::ZERO,
                ..Default::default()
            },
        );
        let evm = factory.evm_factory.create_evm(&mut db, EvmEnv::default());
        let executor = factory.create_executor(evm, ctx);

        let sig = Signature::new(U256::from(1), U256::from(2), true);

        // Wrap legacy1
        let tx1 =
            Recovered::new_unchecked(EthereumTxEnvelope::Legacy(legacy1.into_signed(sig)), sender);

        // Wrap legacy2
        let tx2 =
            Recovered::new_unchecked(EthereumTxEnvelope::Legacy(legacy2.into_signed(sig)), sender);
        let tx_with_encoded1 = WithEncoded::new(tx1.encoded_2718().into(), tx1);
        let tx_with_encoded2 = WithEncoded::new(tx2.encoded_2718().into(), tx2);

        let _result = executor.execute_block([&tx_with_encoded1, &tx_with_encoded2]).unwrap();
        // println!("{:#?}", _result.block_access_list);

        //  [
        //     AccountChanges {
        //         address: 0x0000000000000000000000000000000000000000,
        //         storage_changes: [],
        //         storage_reads: [],
        //         balance_changes: [
        //             BalanceChange {
        //                 block_access_index: 3,
        //                 post_balance: 5000000000000000000,
        //             },
        //         ],
        //         nonce_changes: [],
        //         code_changes: [],
        //     },
        //     AccountChanges {
        //         address: 0x000000000000000000000000000000000000beef,
        //         storage_changes: [],
        //         storage_reads: [],
        //         balance_changes: [
        //             BalanceChange {
        //                 block_access_index: 1,
        //                 post_balance: 49999899999979000,
        //             },
        //             BalanceChange {
        //                 block_access_index: 2,
        //                 post_balance: 49999799999958000,
        //             },
        //         ],
        //         nonce_changes: [
        //             NonceChange {
        //                 block_access_index: 1,
        //                 new_nonce: 1,
        //             },
        //             NonceChange {
        //                 block_access_index: 2,
        //                 new_nonce: 2,
        //             },
        //         ],
        //         code_changes: [],
        //     },
        //     AccountChanges {
        //         address: 0x000000000000000000000000000000000000dead,
        //         storage_changes: [],
        //         storage_reads: [],
        //         balance_changes: [
        //             BalanceChange {
        //                 block_access_index: 1,
        //                 post_balance: 150000100000000000,
        //             },
        //             BalanceChange {
        //                 block_access_index: 2,
        //                 post_balance: 150000200000000000,
        //             },
        //         ],
        //         nonce_changes: [],
        //         code_changes: [],
        //     },
        // ]
    }
}
