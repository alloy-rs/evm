//! Ethereum EVM implementation.

use crate::{env::EvmEnv, evm::EvmFactory, Database, Evm};
use alloc::vec::Vec;
use alloy_primitives::{Address, Bytes, TxKind, U256};
use core::{
    fmt::Debug, ops::{Deref, DerefMut}
};
use revm::{
    context::{BlockEnv, CfgEnv, TxEnv},
    context_interface::result::{EVMError, HaltReason, ResultAndState},
    interpreter::interpreter::EthInterpreter,
    Context, ExecuteEvm,
};
use revm_inspector::{
    exec::inspect_main, inspector_context::InspectorContext, inspectors::NoOpInspector, Inspector,
};

/// The Ethereum EVM context type.
pub type EthEvmContext<DB> = Context<BlockEnv, TxEnv, CfgEnv, DB>;

/// Ethereum EVM implementation.
#[derive(Debug)]
pub enum EthEvm<DB: Database, I> {
    /// Simple EVM implementation.
    Simple(EthEvmContext<DB>),
    /// EVM with an inspector.
    Inspector(InspectorContext<I, EthEvmContext<DB>>),
}

impl<DB: Database, I> EthEvm<DB, I> {
    /// Provides a reference to the EVM context.
    pub const fn ctx(&self) -> &EthEvmContext<DB> {
        match self {
            Self::Simple(ctx) => ctx,
            Self::Inspector(ctx) => &ctx.inner,
        }
    }

    /// Provides a mutable reference to the EVM context.
    pub fn ctx_mut(&mut self) -> &mut EthEvmContext<DB> {
        match self {
            Self::Simple(ctx) => ctx,
            Self::Inspector(ctx) => &mut ctx.inner,
        }
    }
}

impl<DB: Database, I> Deref for EthEvm<DB, I> {
    type Target = EthEvmContext<DB>;

    fn deref(&self) -> &Self::Target {
        self.ctx()
    }
}

impl<DB: Database, I> DerefMut for EthEvm<DB, I> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.ctx_mut()
    }
}

impl<DB, I> EthEvm<DB, I>
where
    DB: Database,
    I: Inspector<EthEvmContext<DB>, EthInterpreter>,
{
    fn transact(&mut self, tx: TxEnv) -> Result<ResultAndState, EVMError<DB::Error>> {
        match self {
            Self::Simple(ctx) => ctx.exec(tx),
            Self::Inspector(ctx) => inspect_main(ctx),
        }
    }
}

impl<DB, I> Evm for EthEvm<DB, I>
where
    DB: Database,
    I: Inspector<EthEvmContext<DB>, EthInterpreter>,
{
    type DB = DB;
    type Tx = TxEnv;
    type Error = EVMError<DB::Error>;
    type HaltReason = HaltReason;

    fn block(&self) -> &BlockEnv {
        &self.block
    }

    fn transact(&mut self, tx: Self::Tx) -> Result<ResultAndState, Self::Error> {
        self.transact(tx)
    }

    fn transact_system_call(
        &mut self,
        caller: Address,
        contract: Address,
        data: Bytes,
    ) -> Result<ResultAndState, Self::Error> {
        #[allow(clippy::needless_update)] // side-effect of optimism fields
        let tx = TxEnv {
            caller,
            kind: TxKind::Call(contract),
            // Explicitly set nonce to 0 so revm does not do any nonce checks
            nonce: 0,
            gas_limit: 30_000_000,
            value: U256::ZERO,
            data,
            // Setting the gas price to zero enforces that no value is transferred as part of the
            // call, and that the call will not count against the block's gas limit
            gas_price: 0,
            // The chain ID check is not relevant here and is disabled if set to None
            chain_id: None,
            // Setting the gas priority fee to None ensures the effective gas price is derived from
            // the `gas_price` field, which we need to be zero
            gas_priority_fee: None,
            access_list: Vec::new(),
            // blob fields can be None for this tx
            blob_hashes: Vec::new(),
            max_fee_per_blob_gas: 0,
            tx_type: 0,
            authorization_list: Default::default(),
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

impl EvmFactory<EvmEnv> for EthEvmFactory {
    type Evm<DB: Database, I: Inspector<EthEvmContext<DB>, EthInterpreter>> = EthEvm<DB, I>;
    type Context<DB: Database> = Context<BlockEnv, TxEnv, CfgEnv, DB>;
    type Tx = TxEnv;
    type Error<DBError: core::error::Error + Send + Sync + 'static> = EVMError<DBError>;
    type HaltReason = HaltReason;

    fn create_evm<DB: Database>(&self, db: DB, input: EvmEnv) -> Self::Evm<DB, NoOpInspector> {
        EthEvm::Simple(
            Context::default().with_block(input.block_env).with_cfg(input.cfg_env).with_db(db),
        )
    }

    fn create_evm_with_inspector<DB: Database, I: Inspector<Self::Context<DB>, EthInterpreter>>(
        &self,
        db: DB,
        input: EvmEnv,
        inspector: I,
    ) -> Self::Evm<DB, I> {
        let ctx =
            Context::default().with_block(input.block_env).with_cfg(input.cfg_env).with_db(db);
        EthEvm::Inspector(InspectorContext::new(ctx, inspector))
    }
}
