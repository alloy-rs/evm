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
    precompiles: PrecompilesMap,
    /// The spec these precompiles belong to.
    spec: Spec,
}

impl<Spec> SpecPrecompiles<Spec> {
    /// Creates the [`SpecPrecompiles`] from a static reference.
    pub const fn from_static(precompiles: &'static Precompiles, spec: Spec) -> Self {
        Self { precompiles: PrecompilesMap::Builtin(Cow::Borrowed(precompiles)), spec }
    }

    /// Creates a new set of precompiles for a spec.
    pub fn new(precompiles: Cow<'static, Precompiles>, spec: Spec) -> Self {
        Self { precompiles: PrecompilesMap::Builtin(precompiles), spec }
    }

    /// Returns the configured precompiles
    pub fn precompiles(&self) -> &Precompiles {
        todo!()
    }

    /// Returns mutable access to the precompiles.
    pub fn precompiles_mut(&mut self) -> &mut Precompiles {
        match &mut self.precompiles {
            PrecompilesMap::Builtin(cow) => {
                // Ensure we have a mutable Cow by cloning the borrowed data if needed
                if let Cow::Borrowed(static_ref) = cow {
                    *cow = Cow::Owned((*static_ref).clone());
                }
                // Now we definitely have an owned value
                match cow {
                    Cow::Owned(ref mut owned) => owned,
                    Cow::Borrowed(_) => unreachable!("We just ensured we have an owned value"),
                }
            }
            PrecompilesMap::Dynamic(_) => {
                todo!()
            }
        }
    }

    /// Returns the configured spec.
    pub const fn spec(&self) -> &Spec {
        &self.spec
    }

    /// Maps a precompile at the given address using the provided function.
    pub fn map_precompile<F>(&mut self, address: &Address, f: F)
    where
        F: FnOnce(PrecompileFn) -> PrecompileFn + Send + Sync + 'static,
    {
        // Apply a transformation that converts to and from PrecompileFn
        self.apply_precompile(address, |current| None);
    }

    /// Maps all precompiles using the provided function.
    pub fn map_precompiles<F>(&mut self, mut f: F)
    where
        F: FnMut(&Address, DynPrecompile) -> DynPrecompile,
    {
    }

    /// Applies a new precompile at the given address.
    pub fn apply_precompile<F>(&mut self, address: &Address, f: F)
    where
        F: FnOnce(Option<DynPrecompile>) -> Option<DynPrecompile>,
    {
    }
}

impl From<EthPrecompiles> for SpecPrecompiles<SpecId> {
    fn from(value: EthPrecompiles) -> Self {
        Self::from_static(value.precompiles, value.spec)
    }
}

#[derive(Clone)]
enum PrecompilesMap {
    Builtin(Cow<'static, Precompiles>),
    Dynamic(PrecompilesMut),
}

impl core::fmt::Debug for PrecompilesMap {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Builtin(_) => f.debug_struct("PrecompilesMap::Builtin").finish(),
            Self::Dynamic(precompiles) => f
                .debug_struct("PrecompilesMap::Dynamic")
                .field("addresses", &precompiles.addresses)
                .finish(),
        }
    }
}

/// A dynamic precompile implementation that can be modified at runtime.
#[derive(Clone)]
pub struct DynPrecompile(pub(crate) Arc<dyn Precompile + Send + Sync>);

impl core::fmt::Debug for DynPrecompile {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DynPrecompile").finish()
    }
}

impl Precompile for DynPrecompile {
    fn call(&self, data: &Bytes, gas: u64) -> PrecompileResult {
        self.0.call(data, gas)
    }
}

impl From<PrecompileFn> for DynPrecompile {
    fn from(f: PrecompileFn) -> Self {
        // Create a wrapper struct to convert the function pointer to a dynamic trait object
        struct PrecompileFnWrapper(PrecompileFn);

        impl Precompile for PrecompileFnWrapper {
            fn call(&self, data: &Bytes, gas: u64) -> PrecompileResult {
                (self.0)(data, gas)
            }
        }

        Self(Arc::new(PrecompileFnWrapper(f)))
    }
}

impl DynPrecompile {
    /// Wraps this precompile with a custom implementation.
    pub fn wrap<P: Precompile + Send + Sync + 'static>(
        self,
        wrapper: impl FnOnce(Self) -> P,
    ) -> Self {
        Self(Arc::new(wrapper(self)))
    }
}

#[derive(Clone, Default)]
struct PrecompilesMut {
    /// Precompiles
    inner: HashMap<Address, DynPrecompile>,
    /// Addresses of precompile
    addresses: HashSet<Address>,
}

impl core::fmt::Debug for PrecompilesMut {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PrecompilesMut").field("addresses", &self.addresses).finish()
    }
}

/// Trait for implementing precompiled contracts.
pub trait Precompile {
    /// Execute the precompile with the given input data and gas limit.
    fn call(&self, data: &Bytes, gas: u64) -> PrecompileResult;
}
