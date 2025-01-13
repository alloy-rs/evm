use alloy_evm_spec::EthereumHardfork;
use revm::Database;

/// An instance of an ethereum virtual machine.
///
/// An EVM is commonly initialized with the corresponding block context and state and it's only
/// purpose is to execute transactions.
///
/// Executing a transaction will return the outcome of the transaction.
pub trait Evm {
    /// The transaction object that the EVM will execute.
    type Tx;
    /// The outcome of an executed transaction.
    // TODO: this doesn't quite fit `revm's` `ResultAndState` because the state is always separate
    // and only the state is committed.  so perhaps we need `Kind` and `State`, although if we
    // make `commit(&Self::Outcome)` then this should still work.
    type Outcome;
    /// The error type that the EVM can return, in case the transaction execution failed, for
    /// example if the transaction was invalid.
    type Error;

    /// Executes a transaction and returns the outcome.
    fn transact(&mut self, tx: Self::Tx) -> Result<Self::Outcome, Self::Error>;

    fn transact_commit(&mut self, tx: Self::Tx) -> Result<Self::Outcome, Self::Error> {
        let result = self.transact(tx)?;
        self.commit(&result)?;
        Ok(result)
    }

    fn commit(&mut self, state: &Self::Outcome) -> Result<(), Self::Error>;
}

/// A type responsible for creating instances of an ethereum virtual machine given a certain input.
pub trait EvmFactory<Input> {
    /// The EVM type that this factory creates.
    // TODO: this doesn't quite work because this would force use to use an enum approach for trace
    // evm for example, unless we
    type Evm: Evm;

    /// Creates a new instance of an EVM.
    fn create_evm(&self, input: Input) -> Self::Evm;
}

/// An evm that can be configured with a certain tracer.
///
/// TODO: this has the benefit that we can arbitrarily restrict the `EvmFactory::Evm` type to
/// support certain tracers, accesslist, etc...
pub trait TraceEvm<Tracer>: Evm {
    /// Traces a transaction and returns the outcome.
    ///
    /// This expects a mutable reference to the tracer, so the caller retains ownership of the
    /// tracer while the evm populates it when it transacts the transaction.
    fn trace(&mut self, tx: Self::Tx, tracer: &mut Tracer) -> Result<Self::Outcome, Self::Error>;
}

#[cfg(test)]
mod tests {
    use revm::{
        context::{BlockEnv, CfgEnv},
        context_interface::{
            result::{HaltReasonTrait, ResultAndState},
            DatabaseGetter,
        },
        handler::EthHandler,
        Context, DatabaseCommit, EthContext, EvmExec,
    };
    use revm_database::State;

    use super::*;

    // represents revm::Inspector types from the `revm_inspectors` repository
    struct AccessListInspector;
    struct TracingInspector;

    struct EthEvmFactory {
        cfg: CfgEnv,
    }

    struct BlockEnvWithSpecAndDB<DB, HF = EthereumHardfork> {
        block: BlockEnv,
        spec: HF,
        db: DB,
    }

    impl<'a, DB> EvmFactory<BlockEnvWithSpecAndDB<DB>> for EthEvmFactory
    where
        DB: revm::Database,
    {
        type Evm = revm::Evm<revm::Error<State<DB>>, EthContext<State<DB>>>;

        fn create_evm(&self, input: BlockEnvWithSpecAndDB<DB>) -> Self::Evm {
            let db = State::builder()
                .with_database(input.db)
                .with_bundle_update()
                .without_state_clear()
                .build();

            let mut cfg = self.cfg.clone();
            cfg.spec = input.spec.into();

            let ctx =
                Context::builder().with_cfg(self.cfg.clone()).with_block(input.block).with_db(db);

            revm::Evm::new(ctx, EthHandler::default())
        }
    }

    impl<ERROR, CTX, HANDLER, Halt> Evm for revm::Evm<ERROR, CTX, HANDLER>
    where
        Self: EvmExec<Output = Result<ResultAndState<Halt>, ERROR>>,
        Halt: HaltReasonTrait,
        CTX: DatabaseGetter<Database: DatabaseCommit>,
        ERROR: From<<CTX::Database as Database>::Error>,
    {
        type Tx = <Self as EvmExec>::Transaction;
        type Outcome = ResultAndState<Halt>;
        type Error = ERROR;

        fn transact(&mut self, tx: Self::Tx) -> Result<Self::Outcome, Self::Error> {
            self.exec_with_tx(tx)
        }

        fn commit(&mut self, state: &Self::Outcome) -> Result<(), Self::Error> {
            self.context.db().commit(state.state.clone());
            Ok(())
        }
    }

    /// Encapsulates all the tx settings
    struct EthTxEnv;

    trait Primitives {
        type Header: Default;
        type Transaction: Default;
    }

    struct StateProviderBox;

    struct EthApi<E, Prim> {
        factory: E,
        primitives: Prim,
    }

    /// General purpose block input.
    struct BlockInput<H, S> {
        header: H,
        state: S,
        settings: (),
    }

    impl<E, Prim> EthApi<E, Prim>
    where
        Prim: Primitives,
        // TODO: this could probably be simplified with a helper `Eth` trait
        E: EvmFactory<
            BlockInput<Prim::Header, StateProviderBox>,
            Evm: TraceEvm<AccessListInspector> + TraceEvm<TracingInspector>,
        >,
        <E::Evm as Evm>::Tx: From<Prim::Transaction>,
    {
        fn create_access_list(&self) {
            let input = BlockInput {
                header: Prim::Header::default(),
                state: StateProviderBox,
                settings: (),
            };

            let mut tracer = AccessListInspector;
            let mut evm = self.factory.create_evm(input);
            let out = evm.trace(Prim::Transaction::default().into(), &mut tracer);
        }
    }
}
