#![doc = include_str!("../README.md")]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/alloy-rs/core/main/assets/alloy.jpg",
    html_favicon_url = "https://raw.githubusercontent.com/alloy-rs/core/main/assets/favicon.ico"
)]
#![cfg_attr(all(not(test), feature = "optimism"), warn(unused_crate_dependencies))]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![cfg_attr(not(feature = "std"), no_std)]
// The `optimism` feature must be enabled to use this crate.
#![cfg(feature = "optimism")]

use alloc::vec::Vec;
use alloy_evm::{Evm, EvmEnv, EvmFactory};
use alloy_primitives::{Address, Bytes, TxKind, U256};
use core::fmt::Debug;
use revm::{
    inspector_handle_register,
    primitives::{
        BlockEnv, CfgEnvWithHandlerCfg, EVMError, HandlerCfg, OptimismFields, ResultAndState, TxEnv,
    },
    Database, GetInspector,
};

extern crate alloc;

/// OP EVM implementation.
#[derive(derive_more::Debug, derive_more::Deref, derive_more::DerefMut, derive_more::From)]
#[debug(bound(DB::Error: Debug))]
pub struct OpEvm<'a, EXT, DB: Database>(revm::Evm<'a, EXT, DB>);

impl<EXT, DB: Database> Evm for OpEvm<'_, EXT, DB> {
    type DB = DB;
    type Tx = TxEnv;
    type Error = EVMError<DB::Error>;

    fn block(&self) -> &BlockEnv {
        self.0.block()
    }

    fn transact(&mut self, tx: Self::Tx) -> Result<ResultAndState, Self::Error> {
        *self.tx_mut() = tx;
        self.0.transact()
    }

    fn transact_system_call(
        &mut self,
        caller: Address,
        contract: Address,
        data: Bytes,
    ) -> Result<ResultAndState, Self::Error> {
        #[allow(clippy::needless_update)] // side-effect of optimism fields
        let tx_env = TxEnv {
            caller,
            transact_to: TxKind::Call(contract),
            // Explicitly set nonce to None so revm does not do any nonce checks
            nonce: None,
            gas_limit: 30_000_000,
            value: U256::ZERO,
            data,
            // Setting the gas price to zero enforces that no value is transferred as part of the
            // call, and that the call will not count against the block's gas limit
            gas_price: U256::ZERO,
            // The chain ID check is not relevant here and is disabled if set to None
            chain_id: None,
            // Setting the gas priority fee to None ensures the effective gas price is derived from
            // the `gas_price` field, which we need to be zero
            gas_priority_fee: None,
            access_list: Vec::new(),
            // blob fields can be None for this tx
            blob_hashes: Vec::new(),
            max_fee_per_blob_gas: None,
            authorization_list: None,
            optimism: OptimismFields {
                source_hash: None,
                mint: None,
                is_system_transaction: Some(false),
                // The L1 fee is not charged for the EIP-4788 transaction, submit zero bytes for the
                // enveloped tx size.
                enveloped_tx: Some(Bytes::default()),
            },
        };

        *self.tx_mut() = tx_env;

        let mut gas_limit = U256::from(self.tx().gas_limit);
        let mut basefee = U256::ZERO;

        // ensure the block gas limit is >= the tx
        core::mem::swap(&mut self.block_mut().gas_limit, &mut gas_limit);
        // disable the base fee check for this call by setting the base fee to zero
        core::mem::swap(&mut self.block_mut().basefee, &mut basefee);

        let res = self.0.transact();

        // swap back to the previous gas limit
        core::mem::swap(&mut self.block_mut().gas_limit, &mut gas_limit);
        // swap back to the previous base fee
        core::mem::swap(&mut self.block_mut().basefee, &mut basefee);

        res
    }

    fn db_mut(&mut self) -> &mut Self::DB {
        &mut self.context.evm.db
    }
}

/// Factory producing [`OpEvm`].
#[derive(Debug, Default, Clone, Copy)]
#[non_exhaustive]
pub struct OpEvmFactory;

impl EvmFactory<EvmEnv> for OpEvmFactory {
    type Evm<'a, DB: Database + 'a, I: 'a> = OpEvm<'a, I, DB>;

    fn create_evm<'a, DB: Database + 'a>(&self, db: DB, input: EvmEnv) -> Self::Evm<'a, DB, ()> {
        let cfg_env_with_handler_cfg = CfgEnvWithHandlerCfg {
            cfg_env: input.cfg_env,
            handler_cfg: HandlerCfg::new(input.spec),
        };
        OpEvm(
            revm::Evm::builder()
                .with_db(db)
                .with_cfg_env_with_handler_cfg(cfg_env_with_handler_cfg)
                .with_block_env(input.block_env)
                .build(),
        )
    }

    fn create_evm_with_inspector<'a, DB: Database + 'a, I: GetInspector<DB> + 'a>(
        &self,
        db: DB,
        input: EvmEnv,
        inspector: I,
    ) -> Self::Evm<'a, DB, I> {
        let cfg_env_with_handler_cfg = CfgEnvWithHandlerCfg {
            cfg_env: input.cfg_env,
            handler_cfg: HandlerCfg::new(input.spec),
        };
        revm::Evm::builder()
            .with_db(db)
            .with_external_context(inspector)
            .with_cfg_env_with_handler_cfg(cfg_env_with_handler_cfg)
            .with_block_env(input.block_env)
            .append_handler_register(inspector_handle_register)
            .build()
            .into()
    }
}
