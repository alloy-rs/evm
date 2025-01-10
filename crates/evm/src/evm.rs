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
        db::{EmptyDBTyped, State},
        primitives::{
            BlockEnv, CfgEnv, EVMError, Env, EnvWithHandlerCfg, HandlerCfg, ResultAndState, TxEnv,
        },
        DatabaseCommit,
    };

    use super::*;

    // represents revm::Inspector types from the `revm_inspectors` repository
    struct AccessListInspector;
    struct TracingInspector;

    struct EthEvmFactory {
        cfg: CfgEnv,
    }

    struct BlockEnvWithSpec<HF = EthereumHardfork> {
        block: BlockEnv,
        spec: HF,
    }

    trait GetDatabase {
        type Database;

        fn get_database(&self) -> Self::Database;
    }

    trait GetSpec {
        type Spec;

        fn get_spec(&self) -> Self::Spec;
    }

    trait GetBlockEnv {
        type BlockEnv;

        fn get_block_env(&self) -> Self::BlockEnv;
    }

    impl<'a, Input> EvmFactory<Input> for EthEvmFactory
    where
        Input: GetDatabase<Database: revm::Database + 'static>
            + GetSpec<Spec = EthereumHardfork>
            + GetBlockEnv<BlockEnv = BlockEnv>,
    {
        type Evm = revm::Evm<'static, (), State<Input::Database>>;

        fn create_evm(&self, input: Input) -> Self::Evm {
            let database = State::builder()
                .with_database(input.get_database())
                .with_bundle_update()
                .without_state_clear()
                .build();

            let env = Box::new(Env {
                cfg: self.cfg.clone(),
                block: input.get_block_env(),
                tx: Default::default(),
            });
            let env = EnvWithHandlerCfg {
                env,
                handler_cfg: HandlerCfg { spec_id: input.get_spec().into() },
            };

            revm::Evm::builder().with_db(database).with_env_with_handler_cfg(env).build()
        }
    }

    impl<EXT, DB: Database + DatabaseCommit> Evm for revm::Evm<'_, EXT, DB> {
        type Tx = TxEnv;
        type Outcome = ResultAndState;
        type Error = EVMError<DB::Error>;

        fn transact(&mut self, tx: Self::Tx) -> Result<Self::Outcome, Self::Error> {
            self.context.evm.env.tx = tx;
            self.transact()
        }

        fn commit(&mut self, state: &Self::Outcome) -> Result<(), Self::Error> {
            self.db_mut().commit(state.state.clone());
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
