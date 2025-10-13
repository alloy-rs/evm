//! Block executor for Optimism.

use crate::OpEvmFactory;
use alloc::{borrow::Cow, boxed::Box, vec::Vec};
use alloy_consensus::{Eip658Value, Header, Transaction, TxReceipt};
use alloy_eips::{Encodable2718, Typed2718};
use alloy_evm::{
    block::{
        state_changes::{balance_increment_state, post_block_balance_increments},
        BlockExecutionError, BlockExecutionResult, BlockExecutor, BlockExecutorFactory,
        BlockExecutorFor, BlockValidationError, ExecutableTx, OnStateHook,
        StateChangePostBlockSource, StateChangeSource, SystemCaller,
    },
    eth::receipt_builder::ReceiptBuilderCtx,
    Database, Evm, EvmFactory, FromRecoveredTx, FromTxWithEncoded,
};
use alloy_op_hardforks::{OpChainHardforks, OpHardforks};
use alloy_primitives::{Bytes, B256};
use canyon::ensure_create2_deployer;
use op_alloy_consensus::OpDepositReceipt;
use op_revm::{
    constants::{DA_FOOTPRINT_GAS_SCALAR_OFFSET, DA_FOOTPRINT_GAS_SCALAR_SLOT, L1_BLOCK_CONTRACT},
    estimate_tx_compressed_size,
    transaction::deposit::DEPOSIT_TRANSACTION_TYPE,
    OpTransaction,
};
pub use receipt_builder::OpAlloyReceiptBuilder;
use receipt_builder::OpReceiptBuilder;
use revm::{context::result::ResultAndState, database::State, DatabaseCommit, Inspector};

mod canyon;
pub mod receipt_builder;

/// Trait for OP transaction environments. Allows to recover the transaction encoded bytes if
/// they're available.
pub trait OpTxEnv {
    /// Returns the encoded bytes of the transaction.
    fn encoded_bytes(&self) -> Option<&Bytes>;
}

impl<T: revm::context::Transaction> OpTxEnv for OpTransaction<T> {
    fn encoded_bytes(&self) -> Option<&Bytes> {
        self.enveloped_tx.as_ref()
    }
}

/// Context for OP block execution.
#[derive(Debug, Default, Clone)]
pub struct OpBlockExecutionCtx {
    /// Parent block hash.
    pub parent_hash: B256,
    /// Parent beacon block root.
    pub parent_beacon_block_root: Option<B256>,
    /// The block's extra data.
    pub extra_data: Bytes,
}

/// Block executor for Optimism.
#[derive(Debug)]
pub struct OpBlockExecutor<Evm, R: OpReceiptBuilder, Spec> {
    /// Spec.
    pub spec: Spec,
    /// Receipt builder.
    pub receipt_builder: R,
    /// Context for block execution.
    pub ctx: OpBlockExecutionCtx,
    /// The EVM used by executor.
    pub evm: Evm,
    /// Receipts of executed transactions.
    pub receipts: Vec<R::Receipt>,
    /// Total gas used by executed transactions.
    pub gas_used: u64,
    /// Da footprint.
    ///
    /// This is only set for blocks post-Jovian activation.
    /// See [DA footprint block limit spec](https://github.com/ethereum-optimism/specs/blob/main/specs/protocol/jovian/exec-engine.md#da-footprint-block-limit)
    pub da_footprint_used: u64,
    /// Whether Regolith hardfork is active.
    pub is_regolith: bool,
    /// Utility to call system smart contracts.
    pub system_caller: SystemCaller<Spec>,
}

impl<E, R, Spec> OpBlockExecutor<E, R, Spec>
where
    E: Evm,
    R: OpReceiptBuilder,
    Spec: OpHardforks + Clone,
{
    /// Creates a new [`OpBlockExecutor`].
    pub fn new(evm: E, ctx: OpBlockExecutionCtx, spec: Spec, receipt_builder: R) -> Self {
        Self {
            is_regolith: spec
                .is_regolith_active_at_timestamp(evm.block().timestamp.saturating_to()),
            evm,
            system_caller: SystemCaller::new(spec.clone()),
            spec,
            receipt_builder,
            receipts: Vec::new(),
            gas_used: 0,
            da_footprint_used: 0,
            ctx,
        }
    }
}

/// Custom errors that can occur during OP block execution.
#[derive(Debug, thiserror::Error)]
pub enum OpBlockExecutionError {
    /// Failed to load cache account.
    #[error("failed to load cache account")]
    LoadCacheAccount,

    /// Failed to get Jovian da footprint gas scalar from database.
    #[error("failed to get da footprint gas scalar from database: {_0}")]
    GetJovianDaFootprintScalar(Box<dyn core::error::Error + Send + Sync + 'static>),

    /// Transaction DA footprint exceeds available block DA footprint.
    #[error("transaction DA footprint exceeds available block DA footprint")]
    TransactionDaFootprintAboveGasLimit {
        /// The DA footprint of the transaction to execute.
        transaction_da_footprint: u64,
        /// The available block DA footprint.
        available_block_da_footprint: u64,
    },
}

impl<'db, DB, E, R, Spec> OpBlockExecutor<E, R, Spec>
where
    DB: Database + 'db,
    E: Evm<
        DB = &'db mut State<DB>,
        Tx: FromRecoveredTx<R::Transaction> + FromTxWithEncoded<R::Transaction> + OpTxEnv,
    >,
    R: OpReceiptBuilder<Transaction: Transaction + Encodable2718, Receipt: TxReceipt>,
    Spec: OpHardforks,
{
    fn get_jovian_da_footprint_scalar(&mut self) -> Result<u16, BlockExecutionError> {
        let da_footprint_gas_scalar_slot = self
            .evm
            .db_mut()
            .database
            .storage(L1_BLOCK_CONTRACT, DA_FOOTPRINT_GAS_SCALAR_SLOT)
            .map_err(|e| {
                BlockExecutionError::other(OpBlockExecutionError::GetJovianDaFootprintScalar(
                    Box::new(e),
                ))
            })?
            .to_be_bytes::<32>();

        // Extract the first 2 bytes directly as a u16 in big-endian format
        let bytes = [
            da_footprint_gas_scalar_slot[DA_FOOTPRINT_GAS_SCALAR_OFFSET],
            da_footprint_gas_scalar_slot[DA_FOOTPRINT_GAS_SCALAR_OFFSET + 1],
        ];
        Ok(u16::from_be_bytes(bytes))
    }

    fn jovian_da_footprint_estimation(
        &mut self,
        tx: &impl ExecutableTx<Self>,
    ) -> Result<u64, BlockExecutionError> {
        // Try to use the enveloped tx if it exists, otherwise use the encoded 2718 bytes
        let encoded = match tx.to_tx_env().encoded_bytes() {
            Some(encoded) => estimate_tx_compressed_size(encoded),
            None => estimate_tx_compressed_size(tx.tx().encoded_2718().as_ref()),
        };

        Ok(encoded
            .saturating_div(1_000_000)
            .saturating_mul(self.get_jovian_da_footprint_scalar()?.into()))
    }
}

impl<'db, DB, E, R, Spec> BlockExecutor for OpBlockExecutor<E, R, Spec>
where
    DB: Database + 'db,
    E: Evm<
        DB = &'db mut State<DB>,
        Tx: FromRecoveredTx<R::Transaction> + FromTxWithEncoded<R::Transaction> + OpTxEnv,
    >,
    R: OpReceiptBuilder<Transaction: Transaction + Encodable2718, Receipt: TxReceipt>,
    Spec: OpHardforks,
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

        // Ensure that the create2deployer is force-deployed at the canyon transition. Optimism
        // blocks will always have at least a single transaction in them (the L1 info transaction),
        // so we can safely assume that this will always be triggered upon the transition and that
        // the above check for empty blocks will never be hit on OP chains.
        ensure_create2_deployer(
            &self.spec,
            self.evm.block().timestamp.saturating_to(),
            self.evm.db_mut(),
        )
        .map_err(BlockExecutionError::other)?;

        Ok(())
    }

    fn execute_transaction_without_commit(
        &mut self,
        tx: impl ExecutableTx<Self>,
    ) -> Result<ResultAndState<<Self::Evm as Evm>::HaltReason>, BlockExecutionError> {
        let is_deposit = tx.tx().ty() == DEPOSIT_TRANSACTION_TYPE;

        // The sum of the transaction's gas limit, Tg, and the gas utilized in this block prior,
        // must be no greater than the block's gasLimit.
        let block_available_gas = self.evm.block().gas_limit - self.gas_used;
        if tx.tx().gas_limit() > block_available_gas && (self.is_regolith || !is_deposit) {
            return Err(BlockValidationError::TransactionGasLimitMoreThanAvailableBlockGas {
                transaction_gas_limit: tx.tx().gas_limit(),
                block_available_gas,
            }
            .into());
        }

        if self.spec.is_jovian_active_at_timestamp(self.evm.block().timestamp.saturating_to())
            && !is_deposit
        {
            let da_footprint_available = self.evm.block().gas_limit - self.da_footprint_used;

            let tx_da_footprint = self.jovian_da_footprint_estimation(&tx)?;

            if tx_da_footprint > da_footprint_available {
                return Err(BlockExecutionError::Validation(BlockValidationError::Other(
                    Box::new(OpBlockExecutionError::TransactionDaFootprintAboveGasLimit {
                        transaction_da_footprint: tx_da_footprint,
                        available_block_da_footprint: da_footprint_available,
                    }),
                )));
            }
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
        let is_deposit = tx.tx().ty() == DEPOSIT_TRANSACTION_TYPE;

        // Fetch the depositor account from the database for the deposit nonce.
        // Note that this *only* needs to be done post-regolith hardfork, as deposit nonces
        // were not introduced in Bedrock. In addition, regular transactions don't have deposit
        // nonces, so we don't need to touch the DB for those.
        let depositor = (self.is_regolith && is_deposit)
            .then(|| {
                self.evm
                    .db_mut()
                    .load_cache_account(*tx.signer())
                    .map(|acc| acc.account_info().unwrap_or_default())
            })
            .transpose()
            .map_err(BlockExecutionError::other)?;

        self.system_caller.on_state(StateChangeSource::Transaction(self.receipts.len()), &state);

        let gas_used = result.gas_used();

        // append gas used
        self.gas_used += gas_used;

        // Update DA footprint if Jovian is active
        if self.spec.is_jovian_active_at_timestamp(self.evm.block().timestamp.saturating_to())
            && !is_deposit
        {
            let tx_da_footprint = self.jovian_da_footprint_estimation(&tx)?;
            // Add to DA footprint used
            self.da_footprint_used = self.da_footprint_used.saturating_add(tx_da_footprint);
        }

        self.receipts.push(
            match self.receipt_builder.build_receipt(ReceiptBuilderCtx {
                tx: tx.tx(),
                result,
                cumulative_gas_used: self.gas_used,
                evm: &self.evm,
                state: &state,
            }) {
                Ok(receipt) => receipt,
                Err(ctx) => {
                    let receipt = alloy_consensus::Receipt {
                        // Success flag was added in `EIP-658: Embedding transaction status code
                        // in receipts`.
                        status: Eip658Value::Eip658(ctx.result.is_success()),
                        cumulative_gas_used: self.gas_used,
                        logs: ctx.result.into_logs(),
                    };

                    self.receipt_builder.build_deposit_receipt(OpDepositReceipt {
                        inner: receipt,
                        deposit_nonce: depositor.map(|account| account.nonce),
                        // The deposit receipt version was introduced in Canyon to indicate an
                        // update to how receipt hashes should be computed
                        // when set. The state transition process ensures
                        // this is only set for post-Canyon deposit
                        // transactions.
                        deposit_receipt_version: (is_deposit
                            && self.spec.is_canyon_active_at_timestamp(
                                self.evm.block().timestamp.saturating_to(),
                            ))
                        .then_some(1),
                    })
                }
            },
        );

        self.evm.db_mut().commit(state);

        Ok(gas_used)
    }

    fn finish(
        mut self,
    ) -> Result<(Self::Evm, BlockExecutionResult<R::Receipt>), BlockExecutionError> {
        let balance_increments =
            post_block_balance_increments::<Header>(&self.spec, self.evm.block(), &[], None);
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

        let legacy_gas_used =
            self.receipts.last().map(|r| r.cumulative_gas_used()).unwrap_or_default();

        Ok((
            self.evm,
            BlockExecutionResult {
                receipts: self.receipts,
                requests: Default::default(),
                gas_used: legacy_gas_used,
                blob_gas_used: self.da_footprint_used,
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
pub struct OpBlockExecutorFactory<
    R = OpAlloyReceiptBuilder,
    Spec = OpChainHardforks,
    EvmFactory = OpEvmFactory,
> {
    /// Receipt builder.
    receipt_builder: R,
    /// Chain specification.
    spec: Spec,
    /// EVM factory.
    evm_factory: EvmFactory,
}

impl<R, Spec, EvmFactory> OpBlockExecutorFactory<R, Spec, EvmFactory> {
    /// Creates a new [`OpBlockExecutorFactory`] with the given spec, [`EvmFactory`], and
    /// [`OpReceiptBuilder`].
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

impl<R, Spec, EvmF> BlockExecutorFactory for OpBlockExecutorFactory<R, Spec, EvmF>
where
    R: OpReceiptBuilder<Transaction: Transaction + Encodable2718, Receipt: TxReceipt>,
    Spec: OpHardforks,
    EvmF: EvmFactory<
        Tx: FromRecoveredTx<R::Transaction> + FromTxWithEncoded<R::Transaction> + OpTxEnv,
    >,
    Self: 'static,
{
    type EvmFactory = EvmF;
    type ExecutionCtx<'a> = OpBlockExecutionCtx;
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
        OpBlockExecutor::new(evm, ctx, &self.spec, &self.receipt_builder)
    }
}

#[cfg(test)]
mod tests {
    use alloc::{string::ToString, vec};
    use alloy_consensus::{transaction::Recovered, SignableTransaction, TxLegacy};
    use alloy_eips::eip2718::WithEncoded;
    use alloy_evm::EvmEnv;
    use alloy_hardforks::ForkCondition;
    use alloy_op_hardforks::OpHardfork;
    use alloy_primitives::{uint, Address, Signature, U256};
    use op_alloy_consensus::OpTxEnvelope;
    use op_revm::{
        constants::{
            BASE_FEE_SCALAR_OFFSET, ECOTONE_L1_BLOB_BASE_FEE_SLOT, ECOTONE_L1_FEE_SCALARS_SLOT,
            L1_BASE_FEE_SLOT, OPERATOR_FEE_SCALARS_SLOT,
        },
        DefaultOp, L1BlockInfo, OpBuilder, OpSpecId,
    };
    use revm::{
        context::BlockEnv,
        database::{CacheDB, EmptyDB, InMemoryDB},
        inspector::NoOpInspector,
        state::AccountInfo,
        Context,
    };

    use crate::OpEvm;

    use super::*;

    #[test]
    fn test_with_encoded() {
        let executor_factory = OpBlockExecutorFactory::new(
            OpAlloyReceiptBuilder::default(),
            OpChainHardforks::op_mainnet(),
            OpEvmFactory::default(),
        );
        let mut db = State::builder().with_database(CacheDB::<EmptyDB>::default()).build();
        let evm = executor_factory.evm_factory.create_evm(&mut db, EvmEnv::default());
        let mut executor = executor_factory.create_executor(evm, OpBlockExecutionCtx::default());
        let tx = Recovered::new_unchecked(
            OpTxEnvelope::Legacy(TxLegacy::default().into_signed(Signature::new(
                Default::default(),
                Default::default(),
                Default::default(),
            ))),
            Address::ZERO,
        );
        let tx_with_encoded = WithEncoded::new(tx.encoded_2718().into(), tx.clone());

        // make sure we can use both `WithEncoded` and transaction itself as inputs.
        let _ = executor.execute_transaction(&tx);
        let _ = executor.execute_transaction(&tx_with_encoded);
    }

    fn prepare_jovian_db(da_footprint_gas_scalar: u16) -> State<InMemoryDB> {
        const L1_BASE_FEE: U256 = uint!(1_U256);
        const L1_BLOB_BASE_FEE: U256 = uint!(2_U256);
        const L1_BASE_FEE_SCALAR: u64 = 3;
        const L1_BLOB_BASE_FEE_SCALAR: u64 = 4;
        const L1_FEE_SCALARS: U256 = U256::from_limbs([
            0,
            (L1_BASE_FEE_SCALAR << (64 - BASE_FEE_SCALAR_OFFSET * 2)) | L1_BLOB_BASE_FEE_SCALAR,
            0,
            0,
        ]);
        const OPERATOR_FEE_SCALAR: u64 = 5;
        const OPERATOR_FEE_CONST: u64 = 6;
        const OPERATOR_FEE: U256 =
            U256::from_limbs([OPERATOR_FEE_CONST, OPERATOR_FEE_SCALAR, 0, 0]);

        let mut da_footprint_scalar_bytes = [0; 8];
        da_footprint_scalar_bytes[0] = (da_footprint_gas_scalar >> 8) as u8;
        da_footprint_scalar_bytes[1] = da_footprint_gas_scalar as u8;

        let da_footprint_gas_scalar_u64: u64 = u64::from_be_bytes(da_footprint_scalar_bytes);
        let da_footprint_gas_scalar_slot_value: U256 =
            U256::from_limbs([0, 0, 0, da_footprint_gas_scalar_u64]);

        let mut db = State::builder().with_database(InMemoryDB::default()).build();

        db.database.insert_account_info(L1_BLOCK_CONTRACT, AccountInfo { ..Default::default() });

        db.database
            .insert_account_storage(L1_BLOCK_CONTRACT, L1_BASE_FEE_SLOT, L1_BASE_FEE)
            .unwrap();
        db.database
            .insert_account_storage(
                L1_BLOCK_CONTRACT,
                ECOTONE_L1_BLOB_BASE_FEE_SLOT,
                L1_BLOB_BASE_FEE,
            )
            .unwrap();
        db.database
            .insert_account_storage(L1_BLOCK_CONTRACT, ECOTONE_L1_FEE_SCALARS_SLOT, L1_FEE_SCALARS)
            .unwrap();
        db.database
            .insert_account_storage(L1_BLOCK_CONTRACT, OPERATOR_FEE_SCALARS_SLOT, OPERATOR_FEE)
            .unwrap();
        db.database
            .insert_account_storage(
                L1_BLOCK_CONTRACT,
                DA_FOOTPRINT_GAS_SCALAR_SLOT,
                da_footprint_gas_scalar_slot_value,
            )
            .unwrap();

        db.database.insert_account_info(
            Address::ZERO,
            AccountInfo { balance: U256::from(400_000_000), ..Default::default() },
        );

        db
    }

    fn build_executor<'a>(
        db: &'a mut State<InMemoryDB>,
        receipt_builder: &'a OpAlloyReceiptBuilder,
        op_chain_hardforks: &'a OpChainHardforks,
        gas_limit: u64,
        jovian_timestamp: u64,
    ) -> OpBlockExecutor<
        OpEvm<&'a mut State<InMemoryDB>, NoOpInspector>,
        &'a OpAlloyReceiptBuilder,
        &'a OpChainHardforks,
    > {
        let ctx = Context::op()
            .with_db(db)
            .with_chain(L1BlockInfo {
                operator_fee_scalar: Some(U256::from(2)),
                operator_fee_constant: Some(U256::from(50)),
                ..Default::default()
            })
            .with_block(BlockEnv {
                timestamp: U256::from(jovian_timestamp),
                gas_limit,
                ..Default::default()
            })
            .modify_cfg_chained(|cfg| cfg.spec = OpSpecId::JOVIAN);

        let evm = OpEvm::new(ctx.build_op_with_inspector(NoOpInspector {}), true);

        OpBlockExecutor::new(
            evm,
            OpBlockExecutionCtx::default(),
            op_chain_hardforks,
            receipt_builder,
        )
    }

    #[test]
    fn test_jovian_da_footprint_estimation() {
        const DA_FOOTPRINT_GAS_SCALAR: u16 = 7;
        const GAS_LIMIT: u64 = 100_000;
        const JOVIAN_TIMESTAMP: u64 = 1746806402;

        let mut db = prepare_jovian_db(DA_FOOTPRINT_GAS_SCALAR);
        let op_chain_hardforks = OpChainHardforks::new(
            OpHardfork::op_mainnet()
                .into_iter()
                .chain(vec![(OpHardfork::Jovian, ForkCondition::Timestamp(JOVIAN_TIMESTAMP))]),
        );

        let receipt_builder = OpAlloyReceiptBuilder::default();
        let mut executor = build_executor(
            &mut db,
            &receipt_builder,
            &op_chain_hardforks,
            GAS_LIMIT,
            JOVIAN_TIMESTAMP,
        );

        let tx_inner = TxLegacy { gas_limit: GAS_LIMIT, ..Default::default() };

        let tx = Recovered::new_unchecked(
            OpTxEnvelope::Legacy(tx_inner.into_signed(Signature::new(
                Default::default(),
                Default::default(),
                Default::default(),
            ))),
            Address::ZERO,
        );

        assert!(executor.da_footprint_used == 0);

        let expected_da_footprint = executor.jovian_da_footprint_estimation(&tx).unwrap();

        // make sure we can use both `WithEncoded` and transaction itself as inputs.
        let res = executor.execute_transaction(&tx);
        assert!(res.is_ok());

        assert!(executor.da_footprint_used == expected_da_footprint);
    }

    #[test]
    fn test_jovian_da_footprint_estimation_out_of_gas() {
        const DA_FOOTPRINT_GAS_SCALAR: u16 = 7;
        const JOVIAN_TIMESTAMP: u64 = 1746806402;
        const GAS_LIMIT: u64 = 100;

        let mut db = prepare_jovian_db(DA_FOOTPRINT_GAS_SCALAR);
        let op_chain_hardforks = OpChainHardforks::new(
            OpHardfork::op_mainnet()
                .into_iter()
                .chain(vec![(OpHardfork::Jovian, ForkCondition::Timestamp(JOVIAN_TIMESTAMP))]),
        );

        let receipt_builder = OpAlloyReceiptBuilder::default();
        let mut executor = build_executor(
            &mut db,
            &receipt_builder,
            &op_chain_hardforks,
            GAS_LIMIT,
            JOVIAN_TIMESTAMP,
        );

        let tx_inner = TxLegacy { gas_limit: GAS_LIMIT, ..Default::default() };

        let tx = Recovered::new_unchecked(
            OpTxEnvelope::Legacy(tx_inner.into_signed(Signature::new(
                Default::default(),
                Default::default(),
                Default::default(),
            ))),
            Address::ZERO,
        );

        assert!(executor.da_footprint_used == 0);

        let expected_da_footprint = executor.jovian_da_footprint_estimation(&tx).unwrap();

        // make sure we can use both `WithEncoded` and transaction itself as inputs.
        let res = executor.execute_transaction(&tx);
        assert!(res.is_err());
        let err = res.unwrap_err();
        match err {
            BlockExecutionError::Validation(BlockValidationError::Other(err)) => {
                assert_eq!(
                    err.to_string(),
                    OpBlockExecutionError::TransactionDaFootprintAboveGasLimit {
                        transaction_da_footprint: expected_da_footprint,
                        available_block_da_footprint: GAS_LIMIT,
                    }
                    .to_string(),
                );
            }
            _ => panic!("expected TransactionDaFootprintAboveGasLimit error"),
        }
    }

    #[test]
    fn test_jovian_da_footprint_estimation_maxed_out_da_footprint() {
        const DA_FOOTPRINT_GAS_SCALAR: u16 = 2000;
        const JOVIAN_TIMESTAMP: u64 = 1746806402;
        const GAS_LIMIT: u64 = 200_000;

        let mut db = prepare_jovian_db(DA_FOOTPRINT_GAS_SCALAR);
        let op_chain_hardforks = OpChainHardforks::new(
            OpHardfork::op_mainnet()
                .into_iter()
                .chain(vec![(OpHardfork::Jovian, ForkCondition::Timestamp(JOVIAN_TIMESTAMP))]),
        );

        let receipt_builder = OpAlloyReceiptBuilder::default();
        let mut executor = build_executor(
            &mut db,
            &receipt_builder,
            &op_chain_hardforks,
            GAS_LIMIT,
            JOVIAN_TIMESTAMP,
        );

        let tx_inner = TxLegacy { gas_limit: GAS_LIMIT, ..Default::default() };

        let tx = Recovered::new_unchecked(
            OpTxEnvelope::Legacy(tx_inner.into_signed(Signature::new(
                Default::default(),
                Default::default(),
                Default::default(),
            ))),
            Address::ZERO,
        );

        assert!(executor.da_footprint_used == 0);

        let expected_da_footprint = executor.jovian_da_footprint_estimation(&tx).unwrap();

        // make sure we can use both `WithEncoded` and transaction itself as inputs.
        let gas_used_tx = executor.execute_transaction(&tx).expect("failed to execute transaction");

        // The gas used when executing the transaction should be the legacy value...
        assert!(gas_used_tx < expected_da_footprint);

        // The gas used when finishing the executor should be the DA footprint since this is higher
        // than the legacy gas used and jovian is active...
        let (_, result) = executor.finish().expect("failed to finish executor");
        assert_eq!(result.blob_gas_used, expected_da_footprint);
        assert_eq!(result.gas_used, gas_used_tx);
        assert!(result.blob_gas_used > result.gas_used);
    }
}
