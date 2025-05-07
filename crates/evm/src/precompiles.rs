//! Helpers for dealing with Precompiles.

use alloc::{borrow::Cow, boxed::Box, string::String, sync::Arc};
use alloy_consensus::transaction::Either;
use alloy_primitives::{
    map::{HashMap, HashSet},
    Address, Bytes,
};
use revm::{
    context::{Cfg, ContextTr, LocalContextTr},
    handler::{EthPrecompiles, PrecompileProvider},
    interpreter::{CallInput, Gas, InputsImpl, InstructionResult, InterpreterResult},
    precompile::{PrecompileError, PrecompileResult, Precompiles},
};

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
    /// Creates the [`PrecompilesMap`] from a static reference.
    pub fn from_static(precompiles: &'static Precompiles) -> Self {
        Self::new(Cow::Borrowed(precompiles))
    }

    /// Creates a new set of precompiles for a spec.
    pub fn new(precompiles: Cow<'static, Precompiles>) -> Self {
        Self::Builtin(precompiles)
    }

    /// Maps a precompile at the given address using the provided function.
    pub fn map_precompile<F>(&mut self, address: &Address, f: F)
    where
        F: FnOnce(DynPrecompile) -> DynPrecompile + Send + Sync + 'static,
    {
        let dyn_precompiles = self.ensure_dynamic_precompiles();

        // get the current precompile at the address
        if let Some(dyn_precompile) = dyn_precompiles.inner.remove(address) {
            // apply the transformation function
            let transformed = f(dyn_precompile);

            // update the precompile at the address
            dyn_precompiles.inner.insert(*address, transformed);
        }
    }

    /// Maps all precompiles using the provided function.
    pub fn map_precompiles<F>(&mut self, mut f: F)
    where
        F: FnMut(&Address, DynPrecompile) -> DynPrecompile,
    {
        let dyn_precompiles = self.ensure_dynamic_precompiles();

        // apply the transformation to each precompile
        let entries = dyn_precompiles.inner.drain();
        let mut new_map = HashMap::with_capacity(entries.size_hint().0);
        for (addr, precompile) in entries {
            let transformed = f(&addr, precompile);
            new_map.insert(addr, transformed);
        }

        dyn_precompiles.inner = new_map;
    }

    /// Applies a new precompile at the given address.
    pub fn apply_precompile<F>(&mut self, address: &Address, f: F)
    where
        F: FnOnce(Option<DynPrecompile>) -> Option<DynPrecompile>,
    {
        let dyn_precompiles = self.ensure_dynamic_precompiles();
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

    /// Ensures that precompiles are in their dynamic representation.
    /// If they are already dynamic, this is a no-op.
    /// Returns a mutable reference to the dynamic precompiles.
    pub fn ensure_dynamic_precompiles(&mut self) -> &mut DynPrecompiles {
        if let Self::Builtin(ref precompiles_cow) = self {
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

            *self = Self::Dynamic(dynamic);
        }

        match self {
            Self::Dynamic(dynamic) => dynamic,
            _ => unreachable!("We just ensured that this is a Dynamic variant"),
        }
    }

    /// Returns an iterator over references to precompile addresses.
    pub fn addresses(&self) -> impl Iterator<Item = &Address> {
        match self {
            Self::Builtin(precompiles) => Either::Left(precompiles.addresses()),
            Self::Dynamic(dyn_precompiles) => Either::Right(dyn_precompiles.addresses.iter()),
        }
    }

    /// Gets a reference to the precompile at the given address.
    pub fn get(&self, address: &Address) -> Option<impl Precompile + '_> {
        match self {
            Self::Builtin(precompiles) => precompiles.get(address).map(Either::Left),
            Self::Dynamic(dyn_precompiles) => dyn_precompiles.inner.get(address).map(Either::Right),
        }
    }
}

impl From<EthPrecompiles> for PrecompilesMap {
    fn from(value: EthPrecompiles) -> Self {
        Self::from_static(value.precompiles)
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

impl<CTX: ContextTr> PrecompileProvider<CTX> for PrecompilesMap {
    type Output = InterpreterResult;

    fn set_spec(&mut self, _spec: <CTX::Cfg as Cfg>::Spec) -> bool {
        false
    }

    fn run(
        &mut self,
        context: &mut CTX,
        address: &Address,
        inputs: &InputsImpl,
        _is_static: bool,
        gas_limit: u64,
    ) -> Result<Option<InterpreterResult>, String> {
        // Get the precompile at the address
        let precompile = self.get(address);

        if precompile.is_none() {
            return Ok(None);
        }

        let mut result = InterpreterResult {
            result: InstructionResult::Return,
            gas: Gas::new(gas_limit),
            output: Bytes::new(),
        };

        // Execute the precompile
        let r;
        let input_bytes = match &inputs.input {
            CallInput::SharedBuffer(range) => {
                match context.local().shared_memory_buffer_slice(range.clone()) {
                    Some(slice) => {
                        r = slice;
                        r.as_ref()
                    }
                    None => &[],
                }
            }
            CallInput::Bytes(bytes) => bytes.0.iter().as_slice(),
        };

        let precompile_result =
            precompile.expect("None case already handled").call(input_bytes, gas_limit);

        match precompile_result {
            Ok(output) => {
                let underflow = result.gas.record_cost(output.gas_used);
                assert!(underflow, "Gas underflow is not possible");
                result.result = InstructionResult::Return;
                result.output = output.bytes;
            }
            Err(PrecompileError::Fatal(e)) => return Err(e),
            Err(e) => {
                result.result = if e.is_oog() {
                    InstructionResult::PrecompileOOG
                } else {
                    InstructionResult::PrecompileError
                };
            }
        };

        Ok(Some(result))
    }

    fn warm_addresses(&self) -> Box<impl Iterator<Item = Address>> {
        Box::new(self.addresses().copied())
    }

    fn contains(&self, address: &Address) -> bool {
        self.get(address).is_some()
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

impl<'a, F> From<F> for DynPrecompile
where
    F: FnOnce(&'a [u8], u64) -> PrecompileResult + Precompile + Send + Sync + 'static,
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
    fn call(&self, data: &[u8], gas: u64) -> PrecompileResult;
}

impl<F> Precompile for F
where
    F: Fn(&[u8], u64) -> PrecompileResult + Send + Sync,
{
    fn call(&self, data: &[u8], gas: u64) -> PrecompileResult {
        self(data, gas)
    }
}

impl Precompile for DynPrecompile {
    fn call(&self, data: &[u8], gas: u64) -> PrecompileResult {
        self.0.call(data, gas)
    }
}

impl Precompile for &DynPrecompile {
    fn call(&self, data: &[u8], gas: u64) -> PrecompileResult {
        self.0.call(data, gas)
    }
}

impl<A: Precompile, B: Precompile> Precompile for Either<A, B> {
    fn call(&self, data: &[u8], gas: u64) -> PrecompileResult {
        match self {
            Self::Left(p) => p.call(data, gas),
            Self::Right(p) => p.call(data, gas),
        }
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
        let mut spec_precompiles = PrecompilesMap::from(eth_precompiles);

        // create a test input for the precompile (identity precompile)
        let identity_address = address!("0x0000000000000000000000000000000000000004");
        let test_input = Bytes::from_static(b"test data");
        let gas_limit = 1000;

        // Ensure we're using dynamic precompiles
        spec_precompiles.ensure_dynamic_precompiles();

        // using the dynamic precompiles interface
        let dyn_precompile = match &spec_precompiles {
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
            |_data: &[u8], _gas: u64| -> PrecompileResult {
                Ok(PrecompileOutput { gas_used: 10, bytes: Bytes::from_static(b"constant value") })
            }
            .into()
        });

        // get the modified precompile and check it
        let dyn_precompile = match &spec_precompiles {
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
        let closure_precompile = |data: &[u8], _gas: u64| -> PrecompileResult {
            let mut output = b"processed: ".to_vec();
            output.extend_from_slice(data.as_ref());
            Ok(PrecompileOutput { gas_used: 15, bytes: Bytes::from(output) })
        };

        let dyn_precompile: DynPrecompile = closure_precompile.into();

        let result = dyn_precompile.call(&test_input, gas_limit).unwrap();
        assert_eq!(result.gas_used, 15);
        assert_eq!(result.bytes, expected_output);
    }

    #[test]
    fn test_get_precompile() {
        let eth_precompiles = EthPrecompiles::default();
        let spec_precompiles = PrecompilesMap::from(eth_precompiles);

        let identity_address = address!("0x0000000000000000000000000000000000000004");
        let test_input = Bytes::from_static(b"test data");
        let gas_limit = 1000;

        let precompile = spec_precompiles.get(&identity_address);
        assert!(precompile.is_some(), "Identity precompile should exist");

        let result = precompile.unwrap().call(&test_input, gas_limit).unwrap();
        assert_eq!(result.bytes, test_input, "Identity precompile should return the input data");

        let nonexistent_address = address!("0x0000000000000000000000000000000000000099");
        assert!(
            spec_precompiles.get(&nonexistent_address).is_none(),
            "Non-existent precompile should not be found"
        );

        let mut dynamic_precompiles = spec_precompiles;
        dynamic_precompiles.ensure_dynamic_precompiles();

        let dyn_precompile = dynamic_precompiles.get(&identity_address);
        assert!(
            dyn_precompile.is_some(),
            "Identity precompile should exist after conversion to dynamic"
        );

        let result = dyn_precompile.unwrap().call(&test_input, gas_limit).unwrap();
        assert_eq!(
            result.bytes, test_input,
            "Identity precompile should return the input data after conversion to dynamic"
        );
    }
}
