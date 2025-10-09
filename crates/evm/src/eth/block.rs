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
use alloy_eips::{eip4895::Withdrawals, eip7685::Requests, Encodable2718};
use alloy_hardforks::EthereumHardfork;
use alloy_primitives::{Log, B256};
use revm::{context_interface::result::ResultAndState, database::State, DatabaseCommit, Inspector};

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
    pub spec: Spec,

    /// Context for block execution.
    pub ctx: EthBlockExecutionCtx<'a>,
    /// Inner EVM.
    pub evm: Evm,
    /// Utility to call system smart contracts.
    pub system_caller: SystemCaller<Spec>,
    /// Receipt builder.
    pub receipt_builder: R,

    /// Receipts of executed transactions.
    pub receipts: Vec<R::Receipt>,
    /// Total gas used by transactions in this block.
    pub gas_used: u64,
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

        self.system_caller.apply_blockhashes_contract_call(self.ctx.parent_hash, &mut self.evm)?;
        self.system_caller
            .apply_beacon_root_contract_call(self.ctx.parent_beacon_block_root, &mut self.evm)?;

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
        self.evm.transact(&tx).map_err(|err| {
            let hash = tx.tx().trie_hash();
            BlockExecutionError::evm(err, hash)
        })
    }

    fn commit_transaction(
        &mut self,
        output: ResultAndState<<Self::Evm as Evm>::HaltReason>,
        tx: impl ExecutableTx<Self>,
    ) -> Result<u64, BlockExecutionError> {
        let ResultAndState { result, state } = output;

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

        // Commit the state changes.
        self.evm.db_mut().commit(state);

        Ok(gas_used)
    }

    fn finish(
        mut self,
    ) -> Result<(Self::Evm, BlockExecutionResult<R::Receipt>), BlockExecutionError> {
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

            requests.extend(self.system_caller.apply_post_execution_changes(&mut self.evm)?);
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

        Ok((
            self.evm,
            BlockExecutionResult { receipts: self.receipts, requests, gas_used: self.gas_used },
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
