#![doc = include_str!("../README.md")]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/alloy-rs/core/main/assets/alloy.jpg",
    html_favicon_url = "https://raw.githubusercontent.com/alloy-rs/core/main/assets/favicon.ico"
)]
#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![cfg_attr(not(feature = "std"), no_std)]

use alloc::vec::Vec;
use alloy_evm::{Database, Evm, EvmEnv, EvmFactory};
use alloy_primitives::{Address, Bytes, TxKind, U256};
use core::{
    fmt::Debug,
    ops::{Deref, DerefMut},
};
use revm::{
    context::{BlockEnv, CfgEnv, TxEnv},
    context_interface::result::{EVMError, ResultAndState},
    interpreter::interpreter::EthInterpreter,
    ExecuteEvm,
};
use revm_inspector::{inspector_context::InspectorContext, inspectors::NoOpInspector, Inspector};
use revm_optimism::{
    api::inspect::inspect_op, context::OpContext, OpSpec, OpTransaction, OpTransactionError,
    OptimismHaltReason,
};

extern crate alloc;

/// OP EVM context type.
pub type OpEvmContext<DB> =
    revm_optimism::context::OpContext<BlockEnv, OpTransaction<TxEnv>, CfgEnv<OpSpec>, DB>;

/// OP EVM implementation.
pub enum OpEvm<DB: Database, I> {
    Simple(OpEvmContext<DB>),
    Inspector(InspectorContext<I, OpEvmContext<DB>>),
}

impl<DB: Database, I> OpEvm<DB, I> {
    /// Provides a reference to the EVM context.
    pub fn ctx(&self) -> &OpEvmContext<DB> {
        match self {
            Self::Simple(ctx) => ctx,
            Self::Inspector(ctx) => &ctx.inner,
        }
    }

    /// Provides a mutable reference to the EVM context.
    pub fn ctx_mut(&mut self) -> &mut OpEvmContext<DB> {
        match self {
            Self::Simple(ctx) => ctx,
            Self::Inspector(ctx) => &mut ctx.inner,
        }
    }
}

impl<DB: Database, I> Deref for OpEvm<DB, I> {
    type Target = OpEvmContext<DB>;

    fn deref(&self) -> &Self::Target {
        self.ctx()
    }
}

impl<DB: Database, I> DerefMut for OpEvm<DB, I> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.ctx_mut()
    }
}

impl<DB, I> OpEvm<DB, I>
where
    DB: Database,
    I: Inspector<OpEvmContext<DB>, EthInterpreter>,
{
    fn transact(
        &mut self,
        tx: OpTransaction<TxEnv>,
    ) -> Result<ResultAndState<OptimismHaltReason>, EVMError<DB::Error, OpTransactionError>> {
        match self {
            Self::Simple(ctx) => ctx.exec(tx),
            Self::Inspector(ctx) => inspect_op(ctx),
        }
    }
}

impl<DB, I> Evm for OpEvm<DB, I>
where
    DB: Database,
    I: Inspector<OpEvmContext<DB>, EthInterpreter>,
{
    type DB = DB;
    type Tx = OpTransaction<TxEnv>;
    type Error = EVMError<DB::Error, OpTransactionError>;
    type HaltReason = OptimismHaltReason;

    fn block(&self) -> &BlockEnv {
        &self.block
    }

    fn transact(&mut self, tx: Self::Tx) -> Result<ResultAndState<Self::HaltReason>, Self::Error> {
        self.transact(tx)
    }

    fn transact_system_call(
        &mut self,
        caller: Address,
        contract: Address,
        data: Bytes,
    ) -> Result<ResultAndState<Self::HaltReason>, Self::Error> {
        #[allow(clippy::needless_update)] // side-effect of optimism fields
        let tx = OpTransaction {
            base: TxEnv {
                caller,
                kind: TxKind::Call(contract),
                // Explicitly set nonce to 0 so revm does not do any nonce checks
                nonce: 0,
                gas_limit: 30_000_000,
                value: U256::ZERO,
                data,
                // Setting the gas price to zero enforces that no value is transferred as part of
                // the call, and that the call will not count against the block's
                // gas limit
                gas_price: 0,
                // The chain ID check is not relevant here and is disabled if set to None
                chain_id: None,
                // Setting the gas priority fee to None ensures the effective gas price is derived
                // from the `gas_price` field, which we need to be zero
                gas_priority_fee: None,
                access_list: Vec::new(),
                // blob fields can be None for this tx
                blob_hashes: Vec::new(),
                max_fee_per_blob_gas: 0,
                tx_type: 0,
                authorization_list: Default::default(),
            },
            // The L1 fee is not charged for the EIP-4788 transaction, submit zero bytes for the
            // enveloped tx size.
            enveloped_tx: Some(Bytes::default()),
            deposit: Default::default(),
        };

        *self.tx_mut() = tx_env;

        let mut gas_limit = tx.gas_limit;
        let mut basefee = U256::ZERO;
        let mut disable_nonce_check = true;

        // ensure the block gas limit is >= the tx
        core::mem::swap(&mut self.block.gas_limit, &mut gas_limit);
        // disable the base fee check for this call by setting the base fee to zero
        core::mem::swap(&mut self.block.basefee, &mut basefee);
        // disable the nonce check
        core::mem::swap(&mut self.cfg.disable_nonce_check, &mut disable_nonce_check);

        let res = self.transact(tx);

        // swap back to the previous gas limit
        core::mem::swap(&mut self.block_mut().gas_limit, &mut gas_limit);
        // swap back to the previous base fee
        core::mem::swap(&mut self.block_mut().basefee, &mut basefee);
        // swap back to the previous nonce check flag
        core::mem::swap(&mut self.cfg.disable_nonce_check, &mut disable_nonce_check);

        res
    }

    fn db_mut(&mut self) -> &mut Self::DB {
        &mut self.journaled_state.database
    }
}

/// Factory producing [`EthEvm`].
#[derive(Debug, Default, Clone, Copy)]
#[non_exhaustive]
pub struct EthEvmFactory;

impl EvmFactory<EvmEnv<OpSpec>> for EthEvmFactory {
    type Evm<DB: Database, I: Inspector<OpEvmContext<DB>, EthInterpreter>> = OpEvm<DB, I>;
    type Context<DB: Database> = OpEvmContext<DB>;
    type Tx = OpTransaction<TxEnv>;
    type HaltReason = OptimismHaltReason;
    type Error<DBError: core::error::Error + Send + Sync + 'static> =
        EVMError<DBError, OpTransactionError>;

    fn create_evm<DB: Database>(
        &self,
        db: DB,
        input: EvmEnv<OpSpec>,
    ) -> Self::Evm<DB, NoOpInspector> {
        let ctx = OpContext(
            OpEvmContext::default()
                .0
                .with_db(db)
                .with_block(input.block_env)
                .with_cfg(input.cfg_env),
        );
        OpEvm::Simple(ctx)
    }

    fn create_evm_with_inspector<DB: Database, I: Inspector<Self::Context<DB>, EthInterpreter>>(
        &self,
        db: DB,
        input: EvmEnv<OpSpec>,
        inspector: I,
    ) -> Self::Evm<DB, I> {
        let ctx = OpContext(
            OpEvmContext::default()
                .0
                .with_db(db)
                .with_block(input.block_env)
                .with_cfg(input.cfg_env),
        );
        OpEvm::Inspector(InspectorContext::new(ctx, inspector))
    }
}
