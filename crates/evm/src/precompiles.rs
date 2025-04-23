//! Helpers for dealing with Precompiles.

use alloc::{borrow::Cow, boxed::Box, string::String, sync::Arc, vec::Vec};
use alloy_primitives::{
    map::{HashMap, HashSet},
    Address, Bytes,
};
use revm::{
    context::{Cfg, ContextTr},
    handler::{EthPrecompiles, PrecompileProvider},
    interpreter::{Gas, InputsImpl, InstructionResult, InterpreterResult},
    precompile::{PrecompileError, PrecompileResult, Precompiles},
};

/// A set of Precompiles according to a spec.
#[derive(Debug, Clone)]
pub struct SpecPrecompiles {
    /// The configured precompiles.
    precompiles: PrecompilesMap,
}

impl SpecPrecompiles {
    /// Creates the [`SpecPrecompiles`] from a static reference.
    pub fn from_static(precompiles: &'static Precompiles) -> Self {
        Self::new(Cow::Borrowed(precompiles))
    }

    /// Creates a new set of precompiles for a spec.
    pub fn new(precompiles: Cow<'static, Precompiles>) -> Self {
        Self { precompiles: PrecompilesMap::Builtin(precompiles) }
    }

    /// Returns the configured precompiles as a read-only reference.
    pub fn precompiles(&self) -> &PrecompilesMap {
        &self.precompiles
    }

    /// Returns mutable access to the precompiles as a DynPrecompiles.
    pub fn precompiles_mut(&mut self) -> &mut PrecompilesMap {
        &mut self.precompiles
    }

    /// Maps a precompile at the given address using the provided function.
    pub fn map_precompile<F>(&mut self, address: &Address, f: F)
    where
        F: FnOnce(DynPrecompile) -> DynPrecompile + Send + Sync + 'static,
    {
        self.ensure_dynamic_precompiles();

        if let PrecompilesMap::Dynamic(ref mut dyn_precompiles) = self.precompiles_mut() {
            let inner = &mut dyn_precompiles.inner;
            // get the current precompile at the address
            if let Some(dyn_precompile) = inner.remove(address) {
                // apply the transformation function
                let transformed = f(dyn_precompile);

                // update the precompile at the address
                inner.insert(*address, transformed);
            }
        }
    }

    /// Maps all precompiles using the provided function.
    pub fn map_precompiles<F>(&mut self, mut f: F)
    where
        F: FnMut(&Address, DynPrecompile) -> DynPrecompile,
    {
        self.ensure_dynamic_precompiles();

        if let PrecompilesMap::Dynamic(ref mut dyn_precompiles) = self.precompiles_mut() {
            // apply the transformation to each precompile
            let mut new_map = HashMap::new();
            for (addr, precompile) in &dyn_precompiles.inner {
                let transformed = f(addr, precompile.clone());
                new_map.insert(*addr, transformed);
            }

            dyn_precompiles.inner = new_map;
        }
    }

    /// Applies a new precompile at the given address.
    pub fn apply_precompile<F>(&mut self, address: &Address, f: F)
    where
        F: FnOnce(Option<DynPrecompile>) -> Option<DynPrecompile>,
    {
        self.ensure_dynamic_precompiles();

        if let PrecompilesMap::Dynamic(ref mut dyn_precompiles) = self.precompiles_mut() {
            let current = dyn_precompiles.inner.get(address).cloned();

            // apply the transformation function
            let result = f(current);

            match result {
                Some(transformed) => {
                    // insert the transformed precompile
                    dyn_precompiles.inner.insert(*address, transformed);
                    dyn_precompiles.addresses.insert(*address);
                }
                None => {
                    // remove the precompile if the transformation returned None
                    dyn_precompiles.inner.remove(address);
                    dyn_precompiles.addresses.remove(address);
                }
            }
        }
    }

    /// Ensures that precompiles are in their dynamic representation.
    /// If they are already dynamic, this is a no-op.
    fn ensure_dynamic_precompiles(&mut self) {
        if let PrecompilesMap::Builtin(ref precompiles_cow) = self.precompiles {
            let mut dynamic = DynPrecompiles::default();

            let static_precompiles = match precompiles_cow {
                Cow::Borrowed(static_ref) => static_ref,
                Cow::Owned(owned) => owned,
            };

            for (addr, precompile_fn) in static_precompiles.inner() {
                let dyn_precompile: DynPrecompile = (*precompile_fn).into();
                dynamic.inner.insert(*addr, dyn_precompile);
                dynamic.addresses.insert(*addr);
            }

            self.precompiles = PrecompilesMap::Dynamic(dynamic);
        }
    }
}

impl From<EthPrecompiles> for SpecPrecompiles {
    fn from(value: EthPrecompiles) -> Self {
        Self::from_static(value.precompiles)
    }
}

// TODO: uncomment when OpPrecompiles exposes precompiles.
//impl From<OpPrecompiles> for SpecPrecompiles {
//    fn from(value: OpPrecompiles) -> Self {
//        Self::from_static(value.precompiles())
//    }
//}

/// A mapping of precompile contracts that can be either static (builtin) or dynamic.
///
/// This is an optimization that allows us to keep using the static precompiles
/// until we need to modify them, at which point we convert to the dynamic representation.
#[derive(Clone)]
pub enum PrecompilesMap {
    /// Static builtin precompiles.
    Builtin(Cow<'static, Precompiles>),
    /// Dynamic precompiles that can be modified at runtime.
    Dynamic(DynPrecompiles),
}

impl PrecompilesMap {
    /// Returns all precompile addresses.
    fn addresses(&self) -> Vec<Address> {
        match self {
            Self::Builtin(precompiles) => precompiles.addresses().copied().collect(),
            Self::Dynamic(dyn_precompiles) => dyn_precompiles.addresses.iter().copied().collect(),
        }
    }
}

impl PartialEq for PrecompilesMap {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Builtin(_), Self::Builtin(_)) => true,
            (Self::Dynamic(a), Self::Dynamic(b)) => a.addresses == b.addresses,
            _ => false,
        }
    }
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

impl SpecPrecompiles {
    /// Handles a precompile call result, updating the interpreter result.
    fn handle_precompile_result(
        result: &mut InterpreterResult,
        precompile_result: PrecompileResult,
    ) -> Result<(), String> {
        match precompile_result {
            Ok(output) => {
                let underflow = result.gas.record_cost(output.gas_used);
                assert!(underflow, "Gas underflow is not possible");
                result.result = InstructionResult::Return;
                result.output = output.bytes;
                Ok(())
            }
            Err(PrecompileError::Fatal(e)) => Err(e),
            Err(e) => {
                result.result = if e.is_oog() {
                    InstructionResult::PrecompileOOG
                } else {
                    InstructionResult::PrecompileError
                };
                Ok(())
            }
        }
    }
}

impl<CTX: ContextTr> PrecompileProvider<CTX> for SpecPrecompiles {
    type Output = InterpreterResult;

    fn set_spec(&mut self, _spec: <CTX::Cfg as Cfg>::Spec) -> bool {
        false
    }

    fn run(
        &mut self,
        _context: &mut CTX,
        address: &Address,
        inputs: &InputsImpl,
        _is_static: bool,
        gas_limit: u64,
    ) -> Result<Option<InterpreterResult>, String> {
        // Check if the address has a precompile
        let has_precompile = match &self.precompiles {
            PrecompilesMap::Builtin(precompiles) => precompiles.contains(address),
            PrecompilesMap::Dynamic(dyn_precompiles) => dyn_precompiles.inner.contains_key(address),
        };

        if !has_precompile {
            return Ok(None);
        }

        let mut result = InterpreterResult {
            result: InstructionResult::Return,
            gas: Gas::new(gas_limit),
            output: Bytes::new(),
        };

        // Get the precompile result based on which variant we have
        let precompile_result = match &self.precompiles {
            PrecompilesMap::Builtin(precompiles) => {
                if let Some(precompile_fn) = precompiles.get(address) {
                    precompile_fn(&inputs.input, gas_limit)
                } else {
                    return Ok(Some(result)); // Should never happen due to has_precompile check
                }
            }
            PrecompilesMap::Dynamic(dyn_precompiles) => {
                if let Some(precompile) = dyn_precompiles.inner.get(address) {
                    precompile.call(&inputs.input, gas_limit)
                } else {
                    return Ok(Some(result)); // Should never happen due to has_precompile check
                }
            }
        };

        // Handle the precompile result
        Self::handle_precompile_result(&mut result, precompile_result)?;

        Ok(Some(result))
    }

    fn warm_addresses(&self) -> Box<impl Iterator<Item = Address>> {
        // Get the addresses from the precompiles and convert to an iterator
        let addresses = self.precompiles.addresses();
        Box::new(addresses.into_iter())
    }

    fn contains(&self, address: &Address) -> bool {
        match &self.precompiles {
            PrecompilesMap::Builtin(precompiles) => precompiles.contains(address),
            PrecompilesMap::Dynamic(dyn_precompiles) => dyn_precompiles.inner.contains_key(address),
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

impl<F> From<F> for DynPrecompile
where
    F: Fn(&Bytes, u64) -> PrecompileResult + Precompile + Send + Sync + 'static,
{
    fn from(f: F) -> Self {
        Self(Arc::new(f))
    }
}

/// A mutable representation of precompiles that allows for runtime modification.
///
/// This structure stores dynamic precompiles that can be modified at runtime,
/// unlike the static `Precompiles` struct from revm.
#[derive(Clone, Default)]
pub struct DynPrecompiles {
    /// Precompiles
    inner: HashMap<Address, DynPrecompile>,
    /// Addresses of precompile
    addresses: HashSet<Address>,
}

impl core::fmt::Debug for DynPrecompiles {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DynPrecompiles").field("addresses", &self.addresses).finish()
    }
}

/// Trait for implementing precompiled contracts.
pub trait Precompile {
    /// Execute the precompile with the given input data and gas limit.
    fn call(&self, data: &Bytes, gas: u64) -> PrecompileResult;
}

impl<F> Precompile for F
where
    F: Fn(&Bytes, u64) -> PrecompileResult + Send + Sync,
{
    fn call(&self, data: &Bytes, gas: u64) -> PrecompileResult {
        self(data, gas)
    }
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

        // Ensure we're using dynamic precompiles
        spec_precompiles.ensure_dynamic_precompiles();

        // using the dynamic precompiles interface
        let precompiles = spec_precompiles.precompiles_mut();
        let dyn_precompile = match precompiles {
            PrecompilesMap::Dynamic(dyn_precompiles) => {
                dyn_precompiles.inner.get(&identity_address).unwrap()
            }
            _ => panic!("Expected dynamic precompiles"),
        };

        let result = dyn_precompile.call(&test_input, gas_limit).unwrap();
        assert_eq!(result.bytes, test_input, "Identity precompile should return the input data");

        // define a function to modify the precompile
        // this will change the identity precompile to always return a fixed value
        let constant_bytes = Bytes::from_static(b"constant value");

        // define a function to modify the precompile to always return a constant value
        spec_precompiles.map_precompile(&identity_address, move |_original_dyn| {
            // create a new DynPrecompile that always returns our constant
            |_data: &Bytes, _gas: u64| -> PrecompileResult {
                Ok(PrecompileOutput { gas_used: 10, bytes: Bytes::from_static(b"constant value") })
            }
            .into()
        });

        // get the modified precompile and check it
        let precompiles = spec_precompiles.precompiles();
        let dyn_precompile = match precompiles {
            PrecompilesMap::Dynamic(dyn_precompiles) => {
                dyn_precompiles.inner.get(&identity_address).unwrap()
            }
            _ => panic!("Expected dynamic precompiles"),
        };

        let result = dyn_precompile.call(&test_input, gas_limit).unwrap();
        assert_eq!(
            result.bytes, constant_bytes,
            "Modified precompile should return the constant value"
        );
    }

    #[test]
    fn test_closure_precompile() {
        let test_input = Bytes::from_static(b"test data");
        let expected_output = Bytes::from_static(b"processed: test data");
        let gas_limit = 1000;

        // define a closure that implements the precompile functionality
        let closure_precompile = |data: &Bytes, _gas: u64| -> PrecompileResult {
            let mut output = b"processed: ".to_vec();
            output.extend_from_slice(data.as_ref());
            Ok(PrecompileOutput { gas_used: 15, bytes: Bytes::from(output) })
        };

        let dyn_precompile: DynPrecompile = closure_precompile.into();

        let result = dyn_precompile.call(&test_input, gas_limit).unwrap();
        assert_eq!(result.gas_used, 15);
        assert_eq!(result.bytes, expected_output);
    }
}
