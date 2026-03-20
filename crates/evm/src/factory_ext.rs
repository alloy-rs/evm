//! Helper extension for `EvmFactory`.

use crate::{evm::EvmFactory, precompiles::PrecompilesMap, Database, Evm};
use core::fmt;
use revm::{inspector::NoOpInspector, Inspector};

/// Trait to encapsulate precompile modification logic.
pub trait ModifyPrecompiles<Spec>: Send + Sync {
    /// Modifies the precompiles map based on the provided specification.
    fn modify_precompiles(&self, spec: Spec, precompiles: &mut PrecompilesMap);
}

// blanket impl for closures to allow ad-hoc modifications
impl<Spec, F> ModifyPrecompiles<Spec> for F
where
    F: Fn(Spec, &mut PrecompilesMap) + Send + Sync,
{
    fn modify_precompiles(&self, spec: Spec, precompiles: &mut PrecompilesMap) {
        (self)(spec, precompiles)
    }
}

/// A wrapper around an existing [`EvmFactory`] that applies a [`ModifyPrecompiles`] modifier.
///
/// This is useful for customizing precompiles (e.g., adding, removing, or wrapping them)
/// in a reusable way.
#[derive(Clone, Copy)]
pub struct EvmFactoryWith<F, M> {
    factory: F,
    modifier: M,
}

impl<F, M> EvmFactoryWith<F, M> {
    /// Creates a new `EvmFactoryWith` instance.
    pub const fn new(factory: F, modifier: M) -> Self {
        Self { factory, modifier }
    }
}

impl<F, M> fmt::Debug for EvmFactoryWith<F, M>
where
    F: fmt::Debug,
    M: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EvmFactoryWith")
            .field("factory", &self.factory)
            .field("modifier", &self.modifier)
            .finish()
    }
}

impl<F, M> EvmFactory for EvmFactoryWith<F, M>
where
    F: EvmFactory<Precompiles = PrecompilesMap>,
    M: ModifyPrecompiles<F::Spec>,
{
    type Evm<DB: Database, I: Inspector<Self::Context<DB>>> = F::Evm<DB, I>;
    type Context<DB: Database> = F::Context<DB>;
    type Tx = F::Tx;
    type Error<DBError: core::error::Error + Send + Sync + 'static> = F::Error<DBError>;
    type HaltReason = F::HaltReason;
    type Spec = F::Spec;
    type BlockEnv = F::BlockEnv;
    type Precompiles = F::Precompiles;

    fn create_evm<DB: Database>(
        &self,
        db: DB,
        input: crate::EvmEnv<Self::Spec, Self::BlockEnv>,
    ) -> Self::Evm<DB, NoOpInspector> {
        let spec = input.cfg_env.spec;
        let mut evm = self.factory.create_evm(db, input);
        self.modifier.modify_precompiles(spec, evm.precompiles_mut());
        evm
    }

    fn create_evm_with_inspector<DB: Database, I: Inspector<Self::Context<DB>>>(
        &self,
        db: DB,
        input: crate::EvmEnv<Self::Spec, Self::BlockEnv>,
        inspector: I,
    ) -> Self::Evm<DB, I> {
        let spec = input.cfg_env.spec;
        let mut evm = self.factory.create_evm_with_inspector(db, input, inspector);
        self.modifier.modify_precompiles(spec, evm.precompiles_mut());
        evm
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{eth::EthEvmFactory, precompiles::DynPrecompile, EvmEnv};
    use alloy_primitives::{address, Bytes};
    use revm::{
        context::{BlockEnv, CfgEnv},
        database_interface::EmptyDB,
        inspector::NoOpInspector,
        precompile::PrecompileId,
        primitives::hardfork::SpecId,
    };

    #[test]
    fn test_factory_with_modifier() {
        let dummy_address = address!("0x00000000000000000000000000000000000000de");

        // A modifier that adds a dummy precompile
        let modifier = |_: SpecId, map: &mut PrecompilesMap| {
            map.apply_precompile(&dummy_address, |_| {
                // Insert a simple precompile that always returns success
                Some(DynPrecompile::new(PrecompileId::custom("test"), |input| {
                    Ok(revm::precompile::PrecompileOutput::new(input.gas, Bytes::from("success")))
                }))
            });
        };

        let factory = EvmFactoryWith::new(EthEvmFactory::default(), modifier);

        let mut cfg = CfgEnv::default();
        cfg.spec = SpecId::CANCUN;
        let env = EvmEnv { block_env: BlockEnv::default(), cfg_env: cfg };

        let mut evm = factory.create_evm(EmptyDB::default(), env);

        // Verify the precompile is present
        assert!(evm.precompiles_mut().get(&dummy_address).is_some());
    }

    #[test]
    fn test_factory_with_closure_modifier() {
        let dummy_address = address!("0x00000000000000000000000000000000000000ff");

        // Directly use a closure as the modifier
        let factory =
            EvmFactoryWith::new(EthEvmFactory::default(), |_: SpecId, map: &mut PrecompilesMap| {
                map.apply_precompile(&dummy_address, |_| {
                    Some(DynPrecompile::new(PrecompileId::custom("test-closure"), |input| {
                        Ok(revm::precompile::PrecompileOutput::new(
                            input.gas,
                            Bytes::from("success-closure"),
                        ))
                    }))
                });
            });

        let mut cfg = CfgEnv::default();
        cfg.spec = SpecId::CANCUN;
        let env = EvmEnv { block_env: BlockEnv::default(), cfg_env: cfg };

        let mut evm = factory.create_evm(EmptyDB::default(), env);

        assert!(evm.precompiles_mut().get(&dummy_address).is_some());
    }

    #[test]
    fn test_factory_with_complex_modifier() {
        let dummy_address = address!("0x00000000000000000000000000000000000000aa");

        // A modifier that behaves differently based on SpecId
        let modifier = |spec: SpecId, map: &mut PrecompilesMap| {
            if spec >= SpecId::CANCUN {
                map.apply_precompile(&dummy_address, |_| {
                    Some(DynPrecompile::new(PrecompileId::custom("cancun-special"), |input| {
                        Ok(revm::precompile::PrecompileOutput::new(
                            input.gas,
                            Bytes::from("cancun"),
                        ))
                    }))
                });
            }
        };

        let factory = EvmFactoryWith::new(EthEvmFactory::default(), modifier);

        // 1. Test with CANCUN
        let mut cfg = CfgEnv::default();
        cfg.spec = SpecId::CANCUN;
        let env = EvmEnv { block_env: BlockEnv::default(), cfg_env: cfg };
        let mut evm = factory.create_evm(EmptyDB::default(), env);
        assert!(evm.precompiles_mut().get(&dummy_address).is_some());

        // 2. Test with LONDON (should NOT have the precompile)
        let mut cfg = CfgEnv::default();
        cfg.spec = SpecId::LONDON;
        let env = EvmEnv { block_env: BlockEnv::default(), cfg_env: cfg };
        let mut evm = factory.create_evm(EmptyDB::default(), env);
        assert!(evm.precompiles_mut().get(&dummy_address).is_none());

        // 3. Test with inspector
        let mut cfg = CfgEnv::default();
        cfg.spec = SpecId::CANCUN;
        let env = EvmEnv { block_env: BlockEnv::default(), cfg_env: cfg };
        let mut evm = factory.create_evm_with_inspector(EmptyDB::default(), env, NoOpInspector);
        assert!(evm.precompiles_mut().get(&dummy_address).is_some());
    }
}
