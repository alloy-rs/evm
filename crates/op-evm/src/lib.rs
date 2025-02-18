#![doc = include_str!("../README.md")]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/alloy-rs/core/main/assets/alloy.jpg",
    html_favicon_url = "https://raw.githubusercontent.com/alloy-rs/core/main/assets/favicon.ico"
)]
#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::vec::Vec;
use alloy_evm::{Database, Evm, EvmEnv, EvmFactory};
use alloy_primitives::{Address, Bytes, TxKind, U256};
use core::{
    fmt::Debug,
    ops::{Deref, DerefMut},
};
use revm::{
    context::{setters::ContextSetters, BlockEnv, TxEnv},
    context_interface::result::{EVMError, ResultAndState},
    handler::instructions::EthInstructions,
    inspector::NoOpInspector,
    interpreter::interpreter::EthInterpreter,
    Context, ExecuteEvm, InspectEvm, Inspector,
};
use revm_optimism::{
    DefaultOp, OpBuilder, OpContext, OpHaltReason, OpSpecId, OpTransaction, OpTransactionError,
};

/// OP EVM implementation.
#[allow(missing_debug_implementations)] // missing revm::OpContext Debug impl
pub struct OpEvm<DB: Database, I> {
    inner: revm_optimism::OpEvm<OpContext<DB>, I, EthInstructions<EthInterpreter, OpContext<DB>>>,
    inspect: bool,
}

impl<DB: Database, I> OpEvm<DB, I> {
    /// Provides a reference to the EVM context.
    pub const fn ctx(&self) -> &OpContext<DB> {
        &self.inner.0.data.ctx
    }

    /// Provides a mutable reference to the EVM context.
    pub fn ctx_mut(&mut self) -> &mut OpContext<DB> {
        &mut self.inner.0.data.ctx
    }
}

impl<DB: Database, I> Deref for OpEvm<DB, I> {
    type Target = OpContext<DB>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.ctx()
    }
}

impl<DB: Database, I> DerefMut for OpEvm<DB, I> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.ctx_mut()
    }
}

impl<DB, I> Evm for OpEvm<DB, I>
where
    DB: Database,
    I: Inspector<OpContext<DB>>,
{
    type DB = DB;
    type Tx = OpTransaction<TxEnv>;
    type Error = EVMError<DB::Error, OpTransactionError>;
    type HaltReason = OpHaltReason;

    fn block(&self) -> &BlockEnv {
        &self.block
    }

    fn transact(&mut self, tx: Self::Tx) -> Result<ResultAndState<Self::HaltReason>, Self::Error> {
        if self.inspect {
            self.inner.set_tx(tx);
            self.inner.inspect_previous()
        } else {
            self.inner.transact(tx)
        }
    }

    fn transact_system_call(
        &mut self,
        caller: Address,
        contract: Address,
        data: Bytes,
    ) -> Result<ResultAndState<Self::HaltReason>, Self::Error> {
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
                access_list: Default::default(),
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

        let mut gas_limit = tx.base.gas_limit;
        let mut basefee = 0;
        let mut disable_nonce_check = true;

        // ensure the block gas limit is >= the tx
        core::mem::swap(&mut self.block.gas_limit, &mut gas_limit);
        // disable the base fee check for this call by setting the base fee to zero
        core::mem::swap(&mut self.block.basefee, &mut basefee);
        // disable the nonce check
        core::mem::swap(&mut self.cfg.disable_nonce_check, &mut disable_nonce_check);

        let res = self.transact(tx);

        // swap back to the previous gas limit
        core::mem::swap(&mut self.block.gas_limit, &mut gas_limit);
        // swap back to the previous base fee
        core::mem::swap(&mut self.block.basefee, &mut basefee);
        // swap back to the previous nonce check flag
        core::mem::swap(&mut self.cfg.disable_nonce_check, &mut disable_nonce_check);

        res
    }

    fn db_mut(&mut self) -> &mut Self::DB {
        &mut self.journaled_state.database
    }
}

/// Factory producing [`OpEvm`]s.
#[derive(Debug, Default, Clone, Copy)]
#[non_exhaustive]
pub struct OpEvmFactory;

impl EvmFactory<EvmEnv<OpSpecId>> for OpEvmFactory {
    type Evm<DB: Database, I: Inspector<OpContext<DB>>> = OpEvm<DB, I>;
    type Context<DB: Database> = OpContext<DB>;
    type Tx = OpTransaction<TxEnv>;
    type Error<DBError: core::error::Error + Send + Sync + 'static> =
        EVMError<DBError, OpTransactionError>;
    type HaltReason = OpHaltReason;

    fn create_evm<DB: Database>(
        &self,
        db: DB,
        input: EvmEnv<OpSpecId>,
    ) -> Self::Evm<DB, NoOpInspector> {
        OpEvm {
            inner: Context::op()
                .with_db(db)
                .with_block(input.block_env)
                .with_cfg(input.cfg_env)
                .build_op_with_inspector(NoOpInspector {}),
            inspect: false,
        }
    }

    fn create_evm_with_inspector<DB: Database, I: Inspector<Self::Context<DB>>>(
        &self,
        db: DB,
        input: EvmEnv<OpSpecId>,
        inspector: I,
    ) -> Self::Evm<DB, I> {
        OpEvm {
            inner: Context::op()
                .with_db(db)
                .with_block(input.block_env)
                .with_cfg(input.cfg_env)
                .build_op_with_inspector(inspector),
            inspect: true,
        }
    }
}
