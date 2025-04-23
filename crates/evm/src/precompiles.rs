//! Helpers for dealing with Precompiles.

use alloc::{borrow::Cow, boxed::Box, string::String, sync::Arc};
use alloy_primitives::{
    map::{HashMap, HashSet},
    Address, Bytes,
};
use revm::{
    context::{Cfg, ContextTr},
    handler::{EthPrecompiles, PrecompileProvider},
    interpreter::{Gas, InputsImpl, InstructionResult, InterpreterResult},
    precompile::{PrecompileError, PrecompileResult, PrecompileSpecId, Precompiles},
    primitives::hardfork::SpecId,
};

/// A set of Precompiles according to a spec.
#[derive(Debug, Clone)]
pub struct SpecPrecompiles<Spec> {
    /// The configured precompiles.
    precompiles: DynPrecompiles,
    /// The spec these precompiles belong to.
    spec: Spec,
}

impl<Spec> SpecPrecompiles<Spec> {
    /// Creates the [`SpecPrecompiles`] from a static reference.
    pub fn from_static(precompiles: &'static Precompiles, spec: Spec) -> Self {
        Self::new(Cow::Borrowed(precompiles), spec)
    }

    /// Creates a new set of precompiles for a spec.
    pub fn new(precompiles: Cow<'static, Precompiles>, spec: Spec) -> Self {
        Self { precompiles: into_dyn_precompiles(precompiles), spec }
    }

    /// Returns the configured precompiles as a read-only reference.
    pub fn precompiles(&self) -> &DynPrecompiles {
        &self.precompiles
    }

    /// Returns mutable access to the precompiles as a DynPrecompiles.
    pub fn precompiles_mut(&mut self) -> &mut DynPrecompiles {
        &mut self.precompiles
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
        let precompiles = self.precompiles_mut();

        // Get the current precompile at the address
        if let Some(dyn_precompile) = precompiles.inner.get(address).cloned() {
            // Apply the transformation function
            let transformed = f(dyn_precompile);

            // Update the precompile at the address
            precompiles.inner.insert(*address, transformed);
        }
    }

    /// Maps all precompiles using the provided function.
    pub fn map_precompiles<F>(&mut self, mut f: F)
    where
        F: FnMut(&Address, DynPrecompile) -> DynPrecompile,
    {
        let precompiles = self.precompiles_mut();

        // apply the transformation to each precompile
        let mut new_map = HashMap::new();
        for (addr, precompile) in &precompiles.inner {
            let transformed = f(addr, precompile.clone());
            new_map.insert(*addr, transformed);
        }

        precompiles.inner = new_map;
    }

    /// Applies a new precompile at the given address.
    pub fn apply_precompile<F>(&mut self, address: &Address, f: F)
    where
        F: FnOnce(Option<DynPrecompile>) -> Option<DynPrecompile>,
    {
        let precompiles = self.precompiles_mut();

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
    }
}

/// Converts the given static precompiles into their dynamic representation.
fn into_dyn_precompiles(precompiles: Cow<'static, Precompiles>) -> DynPrecompiles {
    // Convert static precompiles to dynamic immediately
    let mut dynamic = DynPrecompiles::default();

    // Get the static precompiles
    let static_precompiles = match precompiles {
        Cow::Borrowed(static_ref) => static_ref.clone(),
        Cow::Owned(owned) => owned,
    };

    // Convert all static precompiles to dynamic ones
    for (addr, precompile_fn) in static_precompiles.inner() {
        let dyn_precompile: DynPrecompile = (*precompile_fn).into();
        dynamic.inner.insert(*addr, dyn_precompile);
        dynamic.addresses.insert(*addr);
    }

    dynamic
}

impl From<EthPrecompiles> for SpecPrecompiles<SpecId> {
    fn from(value: EthPrecompiles) -> Self {
        Self::from_static(value.precompiles, value.spec)
    }
}

impl<CTX: ContextTr> PrecompileProvider<CTX> for SpecPrecompiles<<CTX::Cfg as Cfg>::Spec>
where
    <CTX::Cfg as Cfg>::Spec: PartialEq,
{
    type Output = InterpreterResult;

    fn set_spec(&mut self, spec: <CTX::Cfg as Cfg>::Spec) -> bool {
        // generate new precompiles only on new spec
        if spec == self.spec {
            return false;
        }

        self.precompiles = into_dyn_precompiles(Cow::Borrowed(Precompiles::new(
            PrecompileSpecId::from_spec_id(spec.clone().into()),
        )));
        self.spec = spec;
        true
    }

    fn run(
        &mut self,
        _context: &mut CTX,
        address: &Address,
        inputs: &InputsImpl,
        _is_static: bool,
        gas_limit: u64,
    ) -> Result<Option<InterpreterResult>, String> {
        let Some(precompile) = self.precompiles.inner.get(address) else {
            return Ok(None);
        };

        let mut result = InterpreterResult {
            result: InstructionResult::Return,
            gas: Gas::new(gas_limit),
            output: Bytes::new(),
        };

        match precompile.call(&inputs.input, gas_limit) {
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
        }
        Ok(Some(result))
    }

    fn warm_addresses(&self) -> Box<impl Iterator<Item = Address>> {
        Box::new(self.precompiles.addresses.iter().copied())
    }

    fn contains(&self, address: &Address) -> bool {
        self.precompiles.inner.contains_key(address)
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
            |_data: &Bytes, _gas: u64| -> PrecompileResult {
                Ok(PrecompileOutput { gas_used: 10, bytes: Bytes::from_static(b"constant value") })
            }
            .into()
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
