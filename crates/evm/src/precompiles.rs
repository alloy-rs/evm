//! Helpers for dealing with Precompiles.

use alloc::borrow::Cow;
use revm::{handler::EthPrecompiles, precompile::Precompiles, primitives::hardfork::SpecId};

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
}

impl From<EthPrecompiles> for SpecPrecompiles<SpecId> {
    fn from(value: EthPrecompiles) -> Self {
        Self::from_static(value.precompiles, value.spec)
    }
}
