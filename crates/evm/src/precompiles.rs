//! Helpers for dealing with Precompiles.

use alloc::{borrow::Cow, sync::Arc};
use alloy_primitives::{
    map::{HashMap, HashSet},
    Address, Bytes,
};
use revm::{
    handler::EthPrecompiles,
    precompile::{PrecompileFn, PrecompileResult, Precompiles},
    primitives::hardfork::SpecId,
};

/// A set of Precompiles according to a spec.
#[derive(Debug, Clone)]
pub struct SpecPrecompiles<Spec> {
    /// The configured precompiles.
    ///
    /// This is a cow so that this can be modified on demand.
    precompiles: PrecompilesMap,
    /// The spec these precompiles belong to.
    spec: Spec,
}

impl<Spec> SpecPrecompiles<Spec> {
    /// Creates the [`SpecPrecompiles`] from a static reference.
    pub const fn from_static(precompiles: &'static Precompiles, spec: Spec) -> Self {
        // Self { precompiles: Cow::Borrowed(precompiles), spec }
        todo!()
    }

    /// Creates a new set of precompiles for that spec.
    pub fn new(precompiles: Cow<'static, Precompiles>, spec: Spec) -> Self {
        todo!()
    }

    /// Returns the configured precompiles
    pub fn precompiles(&self) -> &Precompiles {
        todo!()
    }

    /// Returns mutable access to the precompiles.
    pub fn precompiles_mut(&mut self) -> &mut Precompiles {
        todo!()
    }

    /// Returns the configured spec.
    pub const fn spec(&self) -> &Spec {
        &self.spec
    }

    pub fn map_precompile<F>(&mut self, address: &Address, f: F)
    where
        F: Fn(PrecompileFn) -> PrecompileFn,
    {
        if let Some(precompile) = self.precompiles_mut().get(address) {}
    }
}

impl From<EthPrecompiles> for SpecPrecompiles<SpecId> {
    fn from(value: EthPrecompiles) -> Self {
        Self::from_static(value.precompiles, value.spec)
    }
}

enum PrecompilesMap {
    Builtin(Cow<'static, Precompiles>),
    Dynamic(PrecompilesMut),
}

#[derive(Clone)]
pub struct DynPrecompile(pub(crate) Arc<dyn Precompile>);

#[derive(Clone, Default)]
struct PrecompilesMut {
    /// Precompiles
    inner: HashMap<Address, DynPrecompile>,
    /// Addresses of precompile
    addresses: HashSet<Address>,
}

pub trait Precompile {
    fn call(&self, data: &Bytes, gas: u64) -> PrecompileResult;
}
