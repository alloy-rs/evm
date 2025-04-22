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

    /// Returns the configured precompiles as a PrecompilesMut
    ///
    /// This converts static Precompiles to a dynamic representation
    /// that can be modified.
    pub fn precompiles(&self) -> PrecompilesMut {
        match &self.precompiles {
            PrecompilesMap::Builtin(cow) => {
                let mut dynamic = PrecompilesMut::default();

                let precompiles = match cow {
                    Cow::Borrowed(static_ref) => *static_ref,
                    Cow::Owned(ref owned) => owned,
                };

                // convert all static precompiles to dynamic ones
                for (addr, precompile_fn) in precompiles.inner() {
                    let dyn_precompile: DynPrecompile = (*precompile_fn).into();
                    dynamic.inner.insert(*addr, dyn_precompile);
                    dynamic.addresses.insert(*addr);
                }

                dynamic
            }
            PrecompilesMap::Dynamic(dynamic) => dynamic.clone(),
        }
    }

    /// Returns mutable access to the precompiles as a PrecompilesMut.
    ///
    /// If the current representation is static, this will convert it to a dynamic
    /// representation and store it for future use. If it's already dynamic, this
    /// will return a new mutable instance with the same data.
    ///
    /// This does not return a direct reference to the internal precompiles, but instead
    /// a new instance that can be modified and then later applied back
    pub fn precompiles_mut(&mut self) -> PrecompilesMut {
        match &self.precompiles {
            PrecompilesMap::Builtin(cow) => {
                let mut dynamic = PrecompilesMut::default();

                let precompiles = match cow {
                    Cow::Borrowed(static_ref) => *static_ref,
                    Cow::Owned(ref owned) => owned,
                };

                // convert all static precompiles to dynamic ones
                for (addr, precompile_fn) in precompiles.inner() {
                    let dyn_precompile: DynPrecompile = (*precompile_fn).into();
                    dynamic.inner.insert(*addr, dyn_precompile);
                    dynamic.addresses.insert(*addr);
                }

                dynamic
            }
            PrecompilesMap::Dynamic(dynamic) => dynamic.clone(),
        }
    }

    /// Sets the precompiles to the given dynamic representation.
    pub fn set_precompiles(&mut self, precompiles: PrecompilesMut) {
        self.precompiles = PrecompilesMap::Dynamic(precompiles);
    }

    /// Returns the configured spec.
    pub const fn spec(&self) -> &Spec {
        &self.spec
    }

    /// Maps a precompile at the given address using the provided function.
    pub fn map_precompile<F>(&mut self, address: &Address, f: F)
    where
        F: FnOnce(DynPrecompile) -> DynPrecompile + Send + Sync + 'static,
    {
        let mut precompiles = self.precompiles_mut();

        match &self.precompiles {
            PrecompilesMap::Builtin(cow) => {
                let static_precompiles = match cow {
                    Cow::Borrowed(static_ref) => *static_ref,
                    Cow::Owned(ref owned) => owned,
                };

                if let Some(precompile_fn) = static_precompiles.get(address) {
                    let dyn_precompile: DynPrecompile = (*precompile_fn).into();

                    let transformed = f(dyn_precompile);

                    precompiles.inner.insert(*address, transformed);
                }
            }
            PrecompilesMap::Dynamic(dynamic) => {
                if let Some(dyn_precompile) = dynamic.inner.get(address).cloned() {
                    let transformed = f(dyn_precompile);

                    precompiles.inner.insert(*address, transformed);
                }
            }
        }

        self.set_precompiles(precompiles);
    }

    /// Maps all precompiles using the provided function.
    pub fn map_precompiles<F>(&mut self, mut f: F)
    where
        F: FnMut(&Address, DynPrecompile) -> DynPrecompile,
    {
        let mut precompiles = self.precompiles_mut();

        // apply the transformation to each precompile
        let mut new_map = HashMap::new();
        for (addr, precompile) in &precompiles.inner {
            let transformed = f(addr, precompile.clone());
            new_map.insert(*addr, transformed);
        }

        precompiles.inner = new_map;

        self.set_precompiles(precompiles);
    }

    /// Applies a new precompile at the given address.
    pub fn apply_precompile<F>(&mut self, address: &Address, f: F)
    where
        F: FnOnce(Option<DynPrecompile>) -> Option<DynPrecompile>,
    {
        let mut precompiles = self.precompiles_mut();

        let current = precompiles.inner.get(address).cloned();

        // apply the transformation function
        let result = f(current);

        match result {
            Some(transformed) => {
                // insert the transformed precompile
                precompiles.inner.insert(*address, transformed);
                precompiles.addresses.insert(*address);
            }
            None => {
                // remove the precompile if the transformation returned None
                precompiles.inner.remove(address);
                precompiles.addresses.remove(address);
            }
        }

        self.set_precompiles(precompiles);
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

/// A mutable representation of precompiles that allows for runtime modification.
///
/// This structure stores dynamic precompiles that can be modified at runtime,
/// unlike the static `Precompiles` struct from revm.
#[derive(Clone, Default)]
pub struct PrecompilesMut {
    /// Precompiles
    pub inner: HashMap<Address, DynPrecompile>,
    /// Addresses of precompile
    pub addresses: HashSet<Address>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{address, Bytes};
    use revm::precompile::PrecompileOutput;

    #[test]
    fn test_map_precompile() {
        let eth_precompiles = EthPrecompiles::default();
        let mut spec_precompiles = SpecPrecompiles::from(eth_precompiles);

        // create a test input for the precompile (identity precompile)
        let identity_address = address!("0x0000000000000000000000000000000000000004");
        let test_input = Bytes::from_static(b"test data");
        let gas_limit = 1000;

        // using the dynamic precompiles interface
        let precompiles = spec_precompiles.precompiles_mut();
        let dyn_precompile = precompiles.inner.get(&identity_address).unwrap();
        let result = dyn_precompile.call(&test_input, gas_limit).unwrap();
        assert_eq!(result.bytes, test_input, "Identity precompile should return the input data");

        // define a function to modify the precompile
        // this will change the identity precompile to always return a fixed value
        let constant_bytes = Bytes::from_static(b"constant value");

        // define a function to modify the precompile to always return a constant value
        spec_precompiles.map_precompile(&identity_address, move |_original_dyn| {
            // create a new DynPrecompile that always returns our constant
            let constant_fn = |_, _| -> PrecompileResult {
                Ok(PrecompileOutput { gas_used: 10, bytes: Bytes::from_static(b"constant value") })
            } as PrecompileFn;

            constant_fn.into()
        });

        // get the modified precompile and check it
        let precompiles = spec_precompiles.precompiles();
        let dyn_precompile = precompiles.inner.get(&identity_address).unwrap();
        let result = dyn_precompile.call(&test_input, gas_limit).unwrap();
        assert_eq!(
            result.bytes, constant_bytes,
            "Modified precompile should return the constant value"
        );
    }
}
