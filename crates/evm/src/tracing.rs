//! Helpers for tracing.

use crate::{Evm, IntoTxEnv};
use core::{fmt::Debug, iter::Peekable};
use revm::{
    context::result::{ExecutionResult, ResultAndState},
    state::EvmState,
    DatabaseCommit,
};

/// A helper type for tracing transactions.
#[derive(Debug, Clone)]
pub struct TxTracer<E: Evm> {
    evm: E,
    fused_inspector: E::Inspector,
}

/// Container type for context exposed in [`TxTracer`].
#[derive(Debug)]
pub struct TracingCtx<'a, T, E: Evm> {
    /// The transaction that was just executed.
    pub tx: T,
    /// Result of transaction execution.
    pub result: ExecutionResult<E::HaltReason>,
    /// State changes after transaction.
    pub state: &'a EvmState,
    /// Inspector state after transaction.
    pub inspector: &'a mut E::Inspector,
    /// Database used when executing the transaction, _before_ committing the state changes.
    pub db: &'a mut E::DB,
    /// Fused inspector.
    fused_inspector: &'a E::Inspector,
    /// Whether the inspector was fused.
    was_fused: &'a mut bool,
}

impl<'a, T, E: Evm<Inspector: Clone>> TracingCtx<'a, T, E> {
    /// Fuses the inspector and returns the current inspector state.
    pub fn take_inspector(&mut self) -> E::Inspector {
        *self.was_fused = true;
        core::mem::replace(self.inspector, self.fused_inspector.clone())
    }
}

impl<E: Evm<Inspector: Clone, DB: DatabaseCommit>> TxTracer<E> {
    /// Creates a new [`TxTracer`] instance.
    pub fn new(mut evm: E) -> Self {
        Self { fused_inspector: evm.inspector_mut().clone(), evm }
    }

    fn fuse_inspector(&mut self) -> E::Inspector {
        core::mem::replace(self.evm.inspector_mut(), self.fused_inspector.clone())
    }

    /// Executes a transaction, and returns its outcome along with the inspector state.
    pub fn trace(
        &mut self,
        tx: impl IntoTxEnv<E::Tx>,
    ) -> Result<TraceOutput<E::HaltReason, E::Inspector>, E::Error> {
        let result = self.evm.transact_commit(tx);
        let inspector = self.fuse_inspector();
        Ok(TraceOutput { result: result?, inspector })
    }

    /// Executes multiple transactions, applies the closure to each transaction result, and returns
    /// the outcomes.
    #[expect(clippy::type_complexity)]
    pub fn trace_many<Txs, T, F, O>(
        &mut self,
        txs: Txs,
        mut f: F,
    ) -> TracerIter<'_, E, Txs::IntoIter, impl FnMut(TracingCtx<'_, T, E>) -> Result<O, E::Error>>
    where
        T: IntoTxEnv<E::Tx> + Clone,
        Txs: IntoIterator<Item = T>,
        F: FnMut(TracingCtx<'_, Txs::Item, E>) -> O,
    {
        self.try_trace_many(txs, move |ctx| Ok(f(ctx)))
    }

    /// Same as [`TxTracer::trace_many`], but operates on closures returning [`Result`]s.
    pub fn try_trace_many<Txs, T, F, O, Err>(
        &mut self,
        txs: Txs,
        hook: F,
    ) -> TracerIter<'_, E, Txs::IntoIter, F>
    where
        T: IntoTxEnv<E::Tx> + Clone,
        Txs: IntoIterator<Item = T>,
        F: FnMut(TracingCtx<'_, T, E>) -> Result<O, Err>,
        Err: From<E::Error>,
    {
        TracerIter {
            inner: self,
            txs: txs.into_iter().peekable(),
            hook,
            skip_last_commit: true,
            fuse: true,
            stopped: false,
        }
    }
}

/// Output of tracing a transaction.
#[derive(Debug, Clone)]
pub struct TraceOutput<H, I> {
    /// Inner EVM output.
    pub result: ExecutionResult<H>,
    /// Inspector state at the end of the execution.
    pub inspector: I,
}

/// Iterator used by tracer.
#[derive(derive_more::Debug)]
#[debug(bound(E::Inspector: Debug))]
pub struct TracerIter<'a, E: Evm, Txs: Iterator, F> {
    inner: &'a mut TxTracer<E>,
    txs: Peekable<Txs>,
    hook: F,
    skip_last_commit: bool,
    fuse: bool,
    /// Set once a transaction fails to execute, so no transaction after it runs.
    stopped: bool,
}

impl<E: Evm, Txs: Iterator, F> TracerIter<'_, E, Txs, F> {
    /// Flips the `skip_last_commit` flag thus making sure all transaction are committed.
    ///
    /// We are skipping last commit by default as it's expected that when tracing users are mostly
    /// interested in tracer output rather than in a state after it.
    pub const fn commit_last_tx(mut self) -> Self {
        self.skip_last_commit = false;
        self
    }

    /// Disables inspector fusing on every transaction and expects user to fuse it manually.
    pub const fn no_fuse(mut self) -> Self {
        self.fuse = false;
        self
    }
}

impl<E, T, Txs, F, O, Err> Iterator for TracerIter<'_, E, Txs, F>
where
    E: Evm<DB: DatabaseCommit, Inspector: Clone>,
    T: IntoTxEnv<E::Tx> + Clone,
    Txs: Iterator<Item = T>,
    Err: From<E::Error>,
    F: FnMut(TracingCtx<'_, T, E>) -> Result<O, Err>,
{
    type Item = Result<O, Err>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.stopped {
            return None;
        }

        let tx = self.txs.next()?;
        let ResultAndState { result, state } = match self.inner.evm.transact(tx.clone()) {
            Ok(result) => result,
            Err(err) => {
                // Yield the error and stop: the transactions after this one would run on a state
                // that is missing it, and dropping the error would make a truncated trace look
                // complete.
                self.stopped = true;
                if self.fuse {
                    self.inner.fuse_inspector();
                }
                return Some(Err(err.into()));
            }
        };

        let TxTracer { evm, fused_inspector } = self.inner;
        let (db, inspector, _) = evm.components_mut();

        let mut was_fused = false;
        let output = (self.hook)(TracingCtx {
            tx,
            result,
            state: &state,
            inspector,
            db,
            fused_inspector: &*fused_inspector,
            was_fused: &mut was_fused,
        });

        // Only commit next transaction if `skip_last_commit` is disabled or there is a next
        // transaction.
        if !self.skip_last_commit || self.txs.peek().is_some() {
            db.commit(state);
        }

        if self.fuse && !was_fused {
            self.inner.fuse_inspector();
        }

        Some(output)
    }
}

#[cfg(test)]
mod tests {
    use crate::{evm::EvmFactoryExt, EthEvmFactory, EvmEnv};
    use alloc::{vec, vec::Vec};
    use core::convert::Infallible;
    use revm::{
        context::{
            result::{EVMError, InvalidTransaction},
            TxEnv,
        },
        database::{CacheDB, EmptyDB},
        inspector::NoOpInspector,
        primitives::{address, Address, TxKind},
    };

    const CALLER: Address = address!("0x1000000000000000000000000000000000000001");

    fn tx(nonce: u64) -> TxEnv {
        TxEnv {
            caller: CALLER,
            nonce,
            gas_limit: 21_000,
            kind: TxKind::Call(CALLER),
            ..Default::default()
        }
    }

    #[test]
    fn try_trace_many_yields_transact_error() {
        let mut tracer = EthEvmFactory::default().create_tracer(
            CacheDB::new(EmptyDB::default()),
            EvmEnv::default(),
            NoOpInspector,
        );

        // tx(5) fails validation with `NonceTooHigh`. The error must reach the caller, and
        // tx(1) must not run after it.
        let results: Vec<_> = tracer
            .try_trace_many([tx(0), tx(5), tx(1)], |ctx| {
                Ok::<_, EVMError<Infallible>>(ctx.result.is_success())
            })
            .collect();

        assert_eq!(
            results,
            vec![
                Ok(true),
                Err(EVMError::Transaction(InvalidTransaction::NonceTooHigh { tx: 5, state: 1 })),
            ]
        );
    }

    #[test]
    fn try_trace_many_collect_returns_transact_error() {
        let mut tracer = EthEvmFactory::default().create_tracer(
            CacheDB::new(EmptyDB::default()),
            EvmEnv::default(),
            NoOpInspector,
        );

        let collected = tracer
            .try_trace_many([tx(0), tx(5), tx(1)], |ctx| {
                Ok::<_, EVMError<Infallible>>(ctx.result.is_success())
            })
            .collect::<Result<Vec<_>, _>>();

        assert_eq!(
            collected,
            Err(EVMError::Transaction(InvalidTransaction::NonceTooHigh { tx: 5, state: 1 }))
        );
    }

    #[test]
    fn trace_many_runs_all_transactions_without_errors() {
        let mut tracer = EthEvmFactory::default().create_tracer(
            CacheDB::new(EmptyDB::default()),
            EvmEnv::default(),
            NoOpInspector,
        );

        let collected = tracer
            .trace_many([tx(0), tx(1), tx(2)], |ctx| ctx.result.is_success())
            .collect::<Result<Vec<_>, _>>();

        assert_eq!(collected, Ok(vec![true, true, true]));
    }
}
