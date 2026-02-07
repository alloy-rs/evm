//! Before/after execution hooks for [`BlockExecutor`].
//!
//! Use [`BlockExecutor::before`] or [`BlockExecutor::after`] to attach hooks, then
//! [`execute_transaction`](BlockExecutor::execute_transaction).
use crate::{
    block::{BlockExecutionError, BlockExecutor, CommitChanges, ExecutableTx, TxResult},
    Evm, RecoveredTx,
};
use alloc::boxed::Box;
use core::fmt;
use revm::context::result::ExecutionResult;

/// Before-tx hook: `(evm, tx) -> HookResult`. Called immediately before execution.
pub type Before<Evm, Tx> = Box<dyn Fn(&mut Evm, &dyn RecoveredTx<Tx>) -> HookResult + Send + Sync>;

/// After-tx hook: `(evm, result) -> HookResult`. Called after execution, before commit.
pub type After<Evm, Halt> =
    Box<dyn Fn(&mut Evm, &ExecutionResult<Halt>) -> HookResult + Send + Sync>;

/// Result type for before/after hooks.
pub type HookResult = Result<(), HookError>;

/// Error from a before/after hook. Use [`BlockExecutionError::other`] to convert.
#[derive(Debug)]
pub struct HookError(pub Box<dyn core::error::Error + Send + Sync + 'static>);

impl core::error::Error for HookError {}
impl fmt::Display for HookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Common closure bound for hooks (Send + Sync + 'static). Use in trait bounds only.
pub trait HookClosure: Send + Sync + 'static {}
impl<T: Send + Sync + 'static> HookClosure for T {}

/// Wraps a [`BlockExecutor`] with before/after hooks (default no-op).
///
/// Hooks are called once per transaction:
/// - **before**: immediately before execution (no state change yet)
/// - **after**: after execution, before commit (return `Err` to veto commit)
///
/// # Example
///
/// ```ignore
/// 
/// let mut executor = factory.create_executor(evm, ctx)
///     .before(|evm, tx| { /* inspect evm/tx */ Ok(()) })
///     .after(|evm, result| { /* inspect result */ Ok(()) });
///
/// executor.execute_tx(&recovered_tx)?;
/// ```
pub struct ExecHook<E: BlockExecutor> {
    inner: E,
    before: Before<E::Evm, E::Transaction>,
    after: After<E::Evm, <E::Evm as Evm>::HaltReason>,
}

impl<E: BlockExecutor> ExecHook<E> {
    /// Creates a wrapper around the given executor with no-op hooks (default).
    pub fn new(inner: E) -> Self {
        Self { inner, before: Box::new(|_, _| Ok(())), after: Box::new(|_, _| Ok(())) }
    }

    /// Sets the before-tx hook. Receives (evm, tx).
    pub fn before<F>(mut self, f: F) -> Self
    where
        F: Fn(&mut E::Evm, &dyn RecoveredTx<E::Transaction>) -> HookResult + HookClosure,
    {
        self.before = Box::new(f);
        self
    }

    /// Sets the after hook. Receives (evm, result).
    pub fn after<F>(mut self, f: F) -> Self
    where
        F: Fn(&mut E::Evm, &ExecutionResult<<E::Evm as Evm>::HaltReason>) -> HookResult
            + HookClosure,
    {
        self.after = Box::new(f);
        self
    }
}

impl<E: BlockExecutor> BlockExecutor for ExecHook<E> {
    type Transaction = E::Transaction;
    type Receipt = E::Receipt;
    type Evm = E::Evm;
    type Result = E::Result;

    fn apply_pre_execution_changes(&mut self) -> Result<(), BlockExecutionError> {
        self.inner.apply_pre_execution_changes()
    }

    fn execute_transaction_with_commit_condition(
        &mut self,
        tx: impl ExecutableTx<Self>,
        f: impl FnOnce(&ExecutionResult<<Self::Evm as Evm>::HaltReason>) -> CommitChanges,
    ) -> Result<Option<u64>, BlockExecutionError> {
        let (tx_env, recovered) = tx.into_parts();

        (self.before)(self.inner.evm_mut(), &recovered).map_err(BlockExecutionError::other)?;

        let output = self.inner.execute_transaction_without_commit((tx_env, recovered))?;

        (self.after)(self.inner.evm_mut(), &output.result().result)
            .map_err(BlockExecutionError::other)?;

        if !f(&output.result().result).should_commit() {
            return Ok(None);
        }

        let gas_used = self.inner.commit_transaction(output)?;
        Ok(Some(gas_used))
    }

    fn execute_transaction_without_commit(
        &mut self,
        tx: impl ExecutableTx<Self>,
    ) -> Result<Self::Result, BlockExecutionError> {
        self.inner.execute_transaction_without_commit(tx)
    }

    fn commit_transaction(&mut self, output: Self::Result) -> Result<u64, BlockExecutionError> {
        self.inner.commit_transaction(output)
    }

    fn finish(
        self,
    ) -> Result<(Self::Evm, crate::block::BlockExecutionResult<Self::Receipt>), BlockExecutionError>
    {
        self.inner.finish()
    }

    fn set_state_hook(&mut self, hook: Option<alloc::boxed::Box<dyn crate::block::OnStateHook>>) {
        self.inner.set_state_hook(hook);
    }

    fn evm_mut(&mut self) -> &mut Self::Evm {
        self.inner.evm_mut()
    }

    fn evm(&self) -> &Self::Evm {
        self.inner.evm()
    }

    fn receipts(&self) -> &[Self::Receipt] {
        self.inner.receipts()
    }
}

impl<E: BlockExecutor + fmt::Debug> fmt::Debug for ExecHook<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecHook").field("inner", &self.inner).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        block::BlockExecutorFactory,
        eth::{
            receipt_builder::AlloyReceiptBuilder, spec::EthSpec, EthBlockExecutionCtx,
            EthBlockExecutorFactory, EthEvmFactory,
        },
        EvmEnv, EvmFactory,
    };
    use alloc::sync::Arc;
    use alloy_consensus::{
        transaction::Recovered, EthereumTxEnvelope, SignableTransaction, TxLegacy,
    };
    use alloy_primitives::{Address, Signature, U256};
    use revm::{database::State, database_interface::EmptyDB};
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn test_tx_hooks() {
        let tx =
            Recovered::new_unchecked(
                EthereumTxEnvelope::Legacy(
                    TxLegacy { gas_limit: 100_000, ..Default::default() }
                        .into_signed(Signature::new(U256::ONE, U256::ONE, false)),
                ),
                Address::ZERO,
            );

        let factory = EthBlockExecutorFactory::new(
            AlloyReceiptBuilder::default(),
            EthSpec::mainnet(),
            EthEvmFactory,
        );
        let mut db = State::builder().with_database(EmptyDB::default()).build();
        let evm = factory.evm_factory().create_evm(&mut db, EvmEnv::default());
        let ctx = EthBlockExecutionCtx {
            parent_hash: Default::default(),
            parent_beacon_block_root: None,
            ommers: &[],
            withdrawals: None,
            extra_data: Default::default(),
            tx_count_hint: None,
        };

        let counts = Arc::new((AtomicU32::new(0), AtomicU32::new(0)));
        let (c_before, c_after) = (Arc::clone(&counts), Arc::clone(&counts));

        let gas_used = factory
            .create_executor(evm, ctx)
            .before(move |_evm, _tx| {
                c_before.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .after(move |_evm, _result| {
                c_after.1.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .execute_transaction(&tx)
            .expect("execute_tx failed");

        assert!(gas_used > 0);
        assert_eq!(counts.0.load(Ordering::SeqCst), 1);
        assert_eq!(counts.1.load(Ordering::SeqCst), 1);
    }
}
