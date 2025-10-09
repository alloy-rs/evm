//! Ethereum block executor.
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
        BlockExecutorFor, BlockValidationError, ExecutableTx, OnStateHook,
        StateChangePostBlockSource, StateChangeSource, SystemCaller,
    },
    Database, Evm, EvmFactory, FromRecoveredTx, FromTxWithEncoded,
};
use alloc::{borrow::Cow, boxed::Box, vec::Vec};
use alloy_consensus::{Header, Transaction, TxReceipt};
use alloy_eips::{
    eip2935::HISTORY_SERVE_WINDOW,
    eip4788::BEACON_ROOTS_ADDRESS,
    eip4895::Withdrawals,
    eip7002::WITHDRAWAL_REQUEST_PREDEPLOY_ADDRESS,
    eip7251::CONSOLIDATION_REQUEST_PREDEPLOY_ADDRESS,
    eip7685::Requests,
    eip7928::{
        balance_change::BalanceChange, AccountChanges, BlockAccessIndex, BlockAccessList,
        SlotChanges, StorageChange,
    },
    Encodable2718,
};
use alloy_hardforks::EthereumHardfork;
use alloy_primitives::{Address, Log, B256, U256};
use revm::{
    context_interface::result::ResultAndState, database::State, primitives::StorageKey,
    state::AccountStatus, DatabaseCommit, Inspector,
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

        let timestamp: u64 = self.evm.block().timestamp.saturating_to();
        if self.spec.is_amsterdam_active_at_timestamp(timestamp) {
            let contract_acc_change = self
                .system_caller
                .apply_blockhashes_contract_call(self.ctx.parent_hash, &mut self.evm)?;
            tracing::debug!("Applied blockhashes contract call, bal {:?}", contract_acc_change);
            if contract_acc_change.is_some() {
                self.block_access_list.as_mut().unwrap().push(contract_acc_change.clone().unwrap());

                tracing::debug!("Pushed blockhashes contract call, bal {:?}", contract_acc_change);
            }

            let pre_beacon = self
                .evm
                .db_mut()
                .database
                .storage(
                    BEACON_ROOTS_ADDRESS,
                    StorageKey::from(timestamp % HISTORY_SERVE_WINDOW as u64),
                )
                .ok();

            let pre_beacon_root = self
                .evm
                .db_mut()
                .database
                .storage(
                    BEACON_ROOTS_ADDRESS,
                    StorageKey::from(
                        (timestamp % HISTORY_SERVE_WINDOW as u64) + HISTORY_SERVE_WINDOW as u64,
                    ),
                )
                .ok();

            let beacon_contract_acc_change = self.system_caller.apply_beacon_root_contract_call(
                self.ctx.parent_beacon_block_root,
                &mut self.evm,
            )?;
            tracing::debug!(
                "Applied beacon root contract call, bal {:?}",
                beacon_contract_acc_change
            );
            if let Some(beacon_contract_acc_changes) = beacon_contract_acc_change {
                let mut account_changes =
                    AccountChanges::default().with_address(BEACON_ROOTS_ADDRESS);

                // slot 0: timestamp % HISTORY_SERVE_WINDOW
                if let Some(change) = beacon_contract_acc_changes.first() {
                    let new_val = change.changes[0].new_value.into();
                    if pre_beacon == Some(new_val) {
                        account_changes
                            .storage_reads
                            .push(StorageKey::from(timestamp % HISTORY_SERVE_WINDOW as u64).into());
                    } else {
                        account_changes.storage_changes.push(
                            SlotChanges::default()
                                .with_slot(
                                    StorageKey::from(timestamp % HISTORY_SERVE_WINDOW as u64)
                                        .into(),
                                )
                                .with_change(StorageChange {
                                    block_access_index: 0,
                                    new_value: new_val.into(),
                                }),
                        );
                    }
                }

                // slot 1: timestamp % HISTORY_SERVE_WINDOW + HISTORY_SERVE_WINDOW
                if let Some(change) = beacon_contract_acc_changes.get(1) {
                    let new_val = change.changes[0].new_value.into();
                    if pre_beacon_root == Some(new_val) {
                        account_changes.storage_reads.push(
                            StorageKey::from(
                                (timestamp % HISTORY_SERVE_WINDOW as u64)
                                    + HISTORY_SERVE_WINDOW as u64,
                            )
                            .into(),
                        );
                    } else {
                        account_changes.storage_changes.push(
                            SlotChanges::default()
                                .with_slot(
                                    StorageKey::from(
                                        (timestamp % HISTORY_SERVE_WINDOW as u64)
                                            + HISTORY_SERVE_WINDOW as u64,
                                    )
                                    .into(),
                                )
                                .with_change(StorageChange {
                                    block_access_index: 0,
                                    new_value: new_val.into(),
                                }),
                        );
                    }
                }

                self.block_access_list.as_mut().unwrap().push(account_changes);
            }
        } else {
            self.system_caller
                .apply_blockhashes_contract_call(self.ctx.parent_hash, &mut self.evm)?;
            self.system_caller.apply_beacon_root_contract_call(
                self.ctx.parent_beacon_block_root,
                &mut self.evm,
            )?;
        }
        Ok(())
    }

    fn execute_transaction_without_commit(
        &mut self,
        tx: impl ExecutableTx<Self>,
    ) -> Result<ResultAndState<<Self::Evm as Evm>::HaltReason>, BlockExecutionError> {
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

        // Execute transaction and return the result
        let res = self.evm.transact(&tx).map_err(|err| {
            let hash = tx.tx().trie_hash();
            BlockExecutionError::evm(err, hash)
        });
        res
    }

    fn commit_transaction(
        &mut self,
        output: ResultAndState<<Self::Evm as Evm>::HaltReason>,
        tx: impl ExecutableTx<Self>,
    ) -> Result<u64, BlockExecutionError> {
        let ResultAndState { result, mut state } = output;

        self.system_caller.on_state(StateChangeSource::Transaction(self.receipts.len()), &state);

        let gas_used = result.gas_used();

        // append gas used
        self.gas_used += gas_used;

        // Push transaction changeset and calculate header bloom filter for receipt.
        self.receipts.push(self.receipt_builder.build_receipt(ReceiptBuilderCtx {
            tx: tx.tx(),
            evm: &self.evm,
            result,
            state: &state,
            cumulative_gas_used: self.gas_used,
        }));

        if self.spec.is_amsterdam_active_at_timestamp(self.evm.block().timestamp.saturating_to()) {
            if let Some(addr) = tx.tx().to() {
                let initial_balance = self
                    .evm
                    .db_mut()
                    .database
                    .basic(addr)
                    .ok()
                    .and_then(|acc| acc.map(|a| a.balance))
                    .unwrap_or(U256::ZERO);

                if let Some(acc) = state.get(&addr) {
                    if let Some(bal) = self.block_access_list.as_mut() {
                        bal.push(crate::eth::utils::from_account_with_tx_index(
                            addr,
                            self.receipts.len() as u64,
                            acc,
                            initial_balance,
                        ));
                        tracing::debug!(
                        "BlockAccessList: CREATE parent contract {:#x}, tx_index={}, storage: {:#?}",
                        addr,
                        self.receipts.len(),
                        acc.storage_access,
                    );
                        state.get_mut(&addr).unwrap().clear_state_changes();
                    }
                }
            }

            if let Some(acc) = state.get(tx.signer()) {
                if *tx.signer() != tx.tx().to().unwrap_or_default() {
                    let initial_balance = self
                        .evm
                        .db_mut()
                        .database
                        .basic(*tx.signer())
                        .ok()
                        .and_then(|acc| acc.map(|a| a.balance))
                        .unwrap_or(U256::ZERO);

                    if let Some(bal) = self.block_access_list.as_mut() {
                        bal.push(crate::eth::utils::from_account_with_tx_index(
                            *tx.signer(),
                            self.receipts.len() as u64,
                            acc,
                            initial_balance,
                        ));
                        tracing::debug!(
                            "BlockAccessList: Tx signer arm tx_index={}, storage: {:#?}",
                            self.receipts.len(),
                            acc.storage_access,
                        );
                        state.get_mut(tx.signer()).unwrap().clear_state_changes();
                    }
                }
            }

            for (address, account) in state.clone().iter() {
                // Skip signer and tx.to()
                if address == tx.signer() || Some(address) == tx.tx().to().as_ref() {
                    continue;
                }

                // Get the initial balance from DB
                let initial_balance = self
                    .evm
                    .db_mut()
                    .database
                    .basic(*address)
                    .ok()
                    .and_then(|acc| acc.map(|a| a.balance))
                    .unwrap_or(U256::ZERO);

                // Check if address is in the access list
                let in_access_list = tx
                    .tx()
                    .access_list()
                    .map(|al| al.flattened().iter().any(|(addr, _)| addr == address))
                    .unwrap_or(false);

                // If address is in access list, require it to be touched
                // If not in access list, push unconditionally
                let should_push = if in_access_list {
                    account.is_touched() && account.status != AccountStatus::default()
                } else {
                    true
                };

                if should_push {
                    if let Some(bal) = self.block_access_list.as_mut() {
                        bal.push(crate::eth::utils::from_account_with_tx_index(
                            *address,
                            self.receipts.len() as u64,
                            account,
                            initial_balance,
                        ));
                    }

                    tracing::debug!(
                    "BlockAccessList: {:#x}, tx_index={}, in_access_list={}, touched={}, pushed={}",
                    address,
                    self.receipts.len(),
                    in_access_list,
                    account.is_touched(),
                    should_push,
                                );

                    state.get_mut(address).unwrap().clear_state_changes();
                }
            }

            tracing::debug!("######## Block : {:?} #########", self.evm.block());
            // // Store access list changes in bal.
            // if let Some(access_list) = tx.tx().access_list() {
            //     for item in &access_list.0 {
            //         let addr = item.address;
            //         let initial_balance = self
            //             .evm
            //             .db_mut()
            //             .database
            //             .basic(addr)
            //             .ok()
            //             .and_then(|acc| acc.map(|a| a.balance))
            //             .unwrap_or(U256::ZERO);

            //         if state.contains_key(&addr) {
            //             let acc = state.get(&addr).unwrap();
            //             if !acc.storage_access.reads.is_empty()
            //                 || !acc.storage_access.writes.is_empty()
            //                 || acc.nonce_change.0 != acc.nonce_change.1
            //                 || acc.balance_change.0 != acc.balance_change.1
            //                 || !acc.code_change.is_empty()
            //             {
            //                 if let Some(bal) = self.block_access_list.as_mut() {
            //                     bal.push(crate::eth::utils::from_account_with_tx_index(
            //                         addr,
            //                         self.receipts.len() as u64,
            //                         state.get(&addr).unwrap(),
            //                         initial_balance,
            //                     ));
            //                     state.get_mut(&addr).unwrap().clear_state_changes();
            //                 }
            //             }
            //         }
            //     }
            // }
        }

        // Commit the state changes.
        self.evm.db_mut().commit(state.clone());
        Ok(gas_used)
    }

    fn finish(
        mut self,
    ) -> Result<(Self::Evm, BlockExecutionResult<R::Receipt>), BlockExecutionError> {
        let post_system_tx = self.receipts.len() + 1;
        let mut post_system_acc_changes: Vec<AccountChanges> = Vec::new();
        let mut pre_withdrawal = Vec::new();
        let mut pre_consolidation = Vec::new();

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
            if self
                .spec
                .is_amsterdam_active_at_timestamp(self.evm.block().timestamp.saturating_to())
            {
                for i in 0..=3 {
                    let value = self
                        .evm
                        .db_mut()
                        .database
                        .storage(WITHDRAWAL_REQUEST_PREDEPLOY_ADDRESS, StorageKey::from(i))
                        .unwrap();
                    pre_withdrawal.push(value);
                }

                for i in 0..=3 {
                    let value = self
                        .evm
                        .db_mut()
                        .database
                        .storage(CONSOLIDATION_REQUEST_PREDEPLOY_ADDRESS, StorageKey::from(i))
                        .unwrap_or_default();
                    pre_consolidation.push(value);
                }
            }
            requests.extend(self.system_caller.apply_post_execution_changes(&mut self.evm)?);
            if self
                .spec
                .is_amsterdam_active_at_timestamp(self.evm.block().timestamp.saturating_to())
            {
                let mut post_withdrawal = Vec::new();
                for i in 0..=3 {
                    let value = self
                        .evm
                        .db_mut()
                        .database
                        .storage(WITHDRAWAL_REQUEST_PREDEPLOY_ADDRESS, StorageKey::from(i))
                        .unwrap_or_default();
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

                post_system_acc_changes.push(
                    super::utils::build_post_execution_system_contract_account_change(
                        WITHDRAWAL_REQUEST_PREDEPLOY_ADDRESS,
                        pre_withdrawal,
                        post_withdrawal,
                        post_system_tx as BlockAccessIndex,
                    ),
                );
                post_system_acc_changes.push(
                    super::utils::build_post_execution_system_contract_account_change(
                        CONSOLIDATION_REQUEST_PREDEPLOY_ADDRESS,
                        pre_consolidation,
                        post_consolidation,
                        post_system_tx as BlockAccessIndex,
                    ),
                );
            }
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

        if self.spec.is_amsterdam_active_at_timestamp(self.evm.block().timestamp.saturating_to()) {
            // All post tx balance increments
            for address in balance_increments.keys() {
                let bal = self.evm.db_mut().database.basic(*address).unwrap().unwrap().balance;
                self.block_access_list.as_mut().unwrap().push(
                    AccountChanges::default().with_address(*address).with_balance_change(
                        BalanceChange {
                            block_access_index: post_system_tx as u64,
                            post_balance: U256::from(bal),
                        },
                    ),
                );
            }

            tracing::debug!("Post tx balance increments: {:#?}", balance_increments);
            // Add post execution system contract account changes
            self.block_access_list.as_mut().unwrap().extend(post_system_acc_changes);
        }
        Ok((
            self.evm,
            BlockExecutionResult {
                receipts: self.receipts,
                requests,
                gas_used: self.gas_used,
                block_access_list: Some(super::utils::sort_and_remove_duplicates_in_bal(
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

    // #[test]
    // fn test_address() {
    //     let addr = address!("0x1a7d50de1c4dc7d5b696f53b65594f21aa55a826");
    //     println!("Address: {:?}", addr);
    //     let cr_addr = alloy_primitives::Address::create(&addr, 0);
    //     println!("Created Address: {:?}", cr_addr);
    // }

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
