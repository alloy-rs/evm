//! Helpers for dealing with Precompiles.

use alloc::borrow::Cow;
use alloy_primitives::Address;
use revm::{handler::EthPrecompiles, precompile::Precompiles, primitives::hardfork::SpecId};
use revm::precompile::PrecompileFn;

/// A set of Precompiles according to a spec.
#[derive(Debug, Clone)]
pub struct SpecPrecompiles<Spec> {
    /// The configured precompiles.
    ///
    /// This is a cow so that this can be modified on demand.
    precompiles: Cow<'static, Precompiles>,
    /// The spec these precompiles belong to.
    spec: Spec,
}


impl<Spec> SpecPrecompiles<Spec> {
    /// Creates the [`SpecPrecompiles`] from a static reference.
    pub const fn from_static(precompiles: &'static Precompiles, spec: Spec) -> Self {
        Self { precompiles: Cow::Borrowed(precompiles), spec }
    }

    /// Creates a new set of precompiles for that spec.
    pub fn new(precompiles: Cow<'static, Precompiles>, spec: Spec) -> Self {
        Self { precompiles, spec }
    }
    
    /// Returns the configured precompiles
    pub fn precompiles(&self) -> &Precompiles {
        &self.precompiles
    }
    
    /// Returns mutable access to the precompiles.
    pub fn precompiles_mut(&mut self) -> &mut Precompiles {
        self.precompiles.to_mut()
    }
    
    /// Returns the configured spec.
    pub const fn spec(&self) -> &Spec {
        &self.spec
    }
    
    pub fn map_precompile<F>(&mut self, address: &Address, f: F) where F: Fn(PrecompileFn) -> PrecompileFn {
        if let Some(precompile) = self.precompiles_mut().get(address) {
            
        }
    }
}

impl From<EthPrecompiles> for SpecPrecompiles<SpecId> {
    fn from(value: EthPrecompiles) -> Self {
        Self::from_static(value.precompiles, value.spec)
    }
}
