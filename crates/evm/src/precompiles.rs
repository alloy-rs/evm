//! Helpers for dealing with Precompiles.

use crate::{Database, EvmInternals};
use alloc::{borrow::Cow, boxed::Box, string::String, sync::Arc};
use alloy_consensus::transaction::Either;
use alloy_primitives::{
    map::{HashMap, HashSet},
    Address, Bytes, U256,
};
use core::fmt::Debug;
use revm::{
    context::LocalContextTr,
    handler::{EthPrecompiles, PrecompileProvider},
    interpreter::{CallInput, Gas, InputsImpl, InstructionResult, InterpreterResult},
    precompile::{PrecompileError, PrecompileFn, PrecompileId, PrecompileResult, Precompiles},
    Context, Journal,
};

/// A mapping of precompile contracts that can be either static (builtin) or dynamic.
///
/// This is an optimization that allows us to keep using the static precompiles
/// until we need to modify them, at which point we convert to the dynamic representation.
#[derive(Clone)]
pub struct PrecompilesMap {
    /// The wrapped precompiles in their current representation.
    precompiles: PrecompilesKind,
    /// An optional dynamic precompile loader that can lookup precompiles dynamically.
    lookup: Option<Arc<dyn PrecompileLookup>>,
}

impl PrecompilesMap {
    /// Creates the [`PrecompilesMap`] from a static reference.
    pub fn from_static(precompiles: &'static Precompiles) -> Self {
        Self::new(Cow::Borrowed(precompiles))
    }

    /// Creates a new set of precompiles for a spec.
    pub fn new(precompiles: Cow<'static, Precompiles>) -> Self {
        Self { precompiles: PrecompilesKind::Builtin(precompiles), lookup: None }
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
    pub fn map_precompiles<F>(&mut self, f: F)
    where
        F: FnMut(&Address, DynPrecompile) -> DynPrecompile,
    {
        self.map_precompiles_filtered(f, |_, _| true);
    }

    /// Maps all pure precompiles using the provided function.
    ///
    /// This is a variant of [`Self::map_precompiles`] that only applies the transformation
    /// to precompiles that are pure, see [`Precompile::is_pure`].
    pub fn map_pure_precompiles<F>(&mut self, f: F)
    where
        F: FnMut(&Address, DynPrecompile) -> DynPrecompile,
    {
        self.map_precompiles_filtered(f, |_, precompile| precompile.is_pure());
    }

    /// Internal helper to map precompiles with an optional filter.
    ///
    /// The `filter` decides whether to apply the mapping function `f` to a given
    /// precompile. If the filter returns `false`, the original precompile is kept.
    #[inline]
    fn map_precompiles_filtered<F, P>(&mut self, mut f: F, mut filter: P)
    where
        F: FnMut(&Address, DynPrecompile) -> DynPrecompile,
        P: FnMut(&Address, &DynPrecompile) -> bool,
    {
        let dyn_precompiles = self.ensure_dynamic_precompiles();

        // apply the transformation to each precompile
        let entries = dyn_precompiles.inner.drain();
        let mut new_map =
            HashMap::with_capacity_and_hasher(entries.size_hint().0, Default::default());
        for (addr, precompile) in entries {
            if filter(&addr, &precompile) {
                let transformed = f(&addr, precompile);
                new_map.insert(addr, transformed);
            } else {
                new_map.insert(addr, precompile);
            }
        }

        dyn_precompiles.inner = new_map;
    }

    /// Applies a transformation to the precompile at the given address.
    ///
    /// This method allows you to add, update, or remove a precompile by applying a closure
    /// to the existing precompile (if any) at the specified address.
    ///
    /// # Behavior
    ///
    /// The closure receives:
    /// - `Some(precompile)` if a precompile exists at the address
    /// - `None` if no precompile exists at the address
    ///
    /// Based on what the closure returns:
    /// - `Some(precompile)` - Insert or replace the precompile at the address
    /// - `None` - Remove the precompile from the address (if it exists)
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Add a new precompile
    /// precompiles.apply_precompile(&address, |_| Some(my_precompile));
    ///
    /// // Update an existing precompile
    /// precompiles.apply_precompile(&address, |existing| {
    ///     existing.map(|p| wrap_with_logging(p))
    /// });
    ///
    /// // Remove a precompile
    /// precompiles.apply_precompile(&address, |_| None);
    ///
    /// // Conditionally update
    /// precompiles.apply_precompile(&address, |existing| {
    ///     if let Some(p) = existing {
    ///         Some(modify_precompile(p))
    ///     } else {
    ///         Some(create_default_precompile())
    ///     }
    /// });
    /// ```
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

    /// Builder-style method that maps a precompile at the given address using the provided
    /// function.
    ///
    /// This is a consuming version of [`map_precompile`](Self::map_precompile) that returns `Self`.
    pub fn with_mapped_precompile<F>(mut self, address: &Address, f: F) -> Self
    where
        F: FnOnce(DynPrecompile) -> DynPrecompile + Send + Sync + 'static,
    {
        self.map_precompile(address, f);
        self
    }

    /// Builder-style method that maps all precompiles using the provided function.
    ///
    /// This is a consuming version of [`map_precompiles`](Self::map_precompiles) that returns
    /// `Self`.
    pub fn with_mapped_precompiles<F>(mut self, f: F) -> Self
    where
        F: FnMut(&Address, DynPrecompile) -> DynPrecompile,
    {
        self.map_precompiles(f);
        self
    }

    /// Builder-style method that applies a transformation to the precompile at the given address.
    ///
    /// This is a consuming version of [`apply_precompile`](Self::apply_precompile) that returns
    /// `Self`. See [`apply_precompile`](Self::apply_precompile) for detailed behavior and
    /// examples.
    pub fn with_applied_precompile<F>(mut self, address: &Address, f: F) -> Self
    where
        F: FnOnce(Option<DynPrecompile>) -> Option<DynPrecompile>,
    {
        self.apply_precompile(address, f);
        self
    }

    /// Sets a dynamic precompile lookup function that is called for addresses not found
    /// in the static precompile map.
    ///
    /// This method allows you to provide runtime-resolved precompiles that aren't known
    /// at initialization time. The lookup function is called whenever a precompile check
    /// is performed for an address that doesn't exist in the main precompile map.
    ///
    /// # Important Notes
    ///
    /// - **Priority**: Static precompiles take precedence. The lookup function is only called if
    ///   the address is not found in the main precompile map.
    /// - **Gas accounting**: Addresses resolved through this lookup are always treated as cold,
    ///   meaning they incur cold access costs even on repeated calls within the same transaction.
    ///   See also [`PrecompileProvider::warm_addresses`].
    /// - **Performance**: The lookup function is called on every precompile check for
    ///   non-registered addresses, so it should be efficient.
    ///
    /// # Example
    ///
    /// ```ignore
    /// precompiles.set_precompile_lookup(|address| {
    ///     // Dynamically resolve precompiles based on address pattern
    ///     if address.as_slice().starts_with(&[0xDE, 0xAD]) {
    ///         Some(DynPrecompile::new(|input| {
    ///             // Custom precompile logic
    ///             Ok(PrecompileOutput {
    ///                 gas_used: 100,
    ///                 bytes: Bytes::from("dynamic precompile"),
    ///             })
    ///         }))
    ///     } else {
    ///         None
    ///     }
    /// });
    /// ```
    pub fn set_precompile_lookup<L>(&mut self, lookup: L)
    where
        L: PrecompileLookup + 'static,
    {
        self.lookup = Some(Arc::new(lookup));
    }

    /// Builder-style method to set a dynamic precompile lookup function.
    ///
    /// This is a consuming version of [`set_precompile_lookup`](Self::set_precompile_lookup)
    /// that returns `Self` for method chaining.
    ///
    /// See [`set_precompile_lookup`](Self::set_precompile_lookup) for detailed behavior,
    /// important notes, and examples.
    pub fn with_precompile_lookup<L>(mut self, lookup: L) -> Self
    where
        L: PrecompileLookup + 'static,
    {
        self.set_precompile_lookup(lookup);
        self
    }

    /// Consumes the type and returns a set of [`DynPrecompile`].
    pub fn into_dyn_precompiles(mut self) -> DynPrecompiles {
        self.ensure_dynamic_precompiles();
        match self.precompiles {
            PrecompilesKind::Dynamic(dynamic) => dynamic,
            _ => unreachable!("We just ensured that this is a Dynamic variant"),
        }
    }

    /// Ensures that precompiles are in their dynamic representation.
    /// If they are already dynamic, this is a no-op.
    /// Returns a mutable reference to the dynamic precompiles.
    pub fn ensure_dynamic_precompiles(&mut self) -> &mut DynPrecompiles {
        if let PrecompilesKind::Builtin(ref precompiles_cow) = self.precompiles {
            let mut dynamic = DynPrecompiles::default();

            let static_precompiles = match precompiles_cow {
                Cow::Borrowed(static_ref) => static_ref,
                Cow::Owned(owned) => owned,
            };

            for (&addr, pc) in static_precompiles.inner().iter() {
                dynamic.inner.insert(addr, DynPrecompile::from(*pc.precompile()));
                dynamic.addresses.insert(addr);
            }

            self.precompiles = PrecompilesKind::Dynamic(dynamic);
        }

        match &mut self.precompiles {
            PrecompilesKind::Dynamic(dynamic) => dynamic,
            _ => unreachable!("We just ensured that this is a Dynamic variant"),
        }
    }

    /// Returns an iterator over the [`PrecompileId`]s of the installed precompiles.
    pub fn identifiers(&self) -> impl Iterator<Item = &PrecompileId> {
        match &self.precompiles {
            PrecompilesKind::Builtin(precompiles) => {
                Either::Left(precompiles.inner().values().map(|p| p.precompile_id()))
            }
            PrecompilesKind::Dynamic(dyn_precompiles) => {
                Either::Right(dyn_precompiles.inner.values().map(|p| p.precompile_id()))
            }
        }
    }

    /// Returns an iterator over references to precompile addresses.
    pub fn addresses(&self) -> impl Iterator<Item = &Address> {
        match &self.precompiles {
            PrecompilesKind::Builtin(precompiles) => Either::Left(precompiles.addresses()),
            PrecompilesKind::Dynamic(dyn_precompiles) => {
                Either::Right(dyn_precompiles.addresses.iter())
            }
        }
    }

    /// Gets a reference to the precompile at the given address.
    ///
    /// This method first checks the static precompile map, and if not found,
    /// falls back to the dynamic lookup function (if set).
    pub fn get(&self, address: &Address) -> Option<impl Precompile + '_> {
        // First check static precompiles
        let static_result = match &self.precompiles {
            PrecompilesKind::Builtin(precompiles) => precompiles.get(address).map(Either::Left),
            PrecompilesKind::Dynamic(dyn_precompiles) => {
                dyn_precompiles.inner.get(address).map(Either::Right)
            }
        };

        // If found in static precompiles, wrap in Left and return
        if let Some(precompile) = static_result {
            return Some(Either::Left(precompile));
        }

        // Otherwise, try the lookup function if available
        let lookup = self.lookup.as_ref()?;
        lookup.lookup(address).map(Either::Right)
    }
}

impl From<EthPrecompiles> for PrecompilesMap {
    fn from(value: EthPrecompiles) -> Self {
        Self::from_static(value.precompiles)
    }
}

impl core::fmt::Debug for PrecompilesMap {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match &self.precompiles {
            PrecompilesKind::Builtin(_) => f.debug_struct("PrecompilesMap::Builtin").finish(),
            PrecompilesKind::Dynamic(precompiles) => f
                .debug_struct("PrecompilesMap::Dynamic")
                .field("addresses", &precompiles.addresses)
                .finish(),
        }
    }
}

impl<BlockEnv, TxEnv, CfgEnv, DB, Chain>
    PrecompileProvider<Context<BlockEnv, TxEnv, CfgEnv, DB, Journal<DB>, Chain>> for PrecompilesMap
where
    BlockEnv: revm::context::Block,
    TxEnv: revm::context::Transaction,
    CfgEnv: revm::context::Cfg,
    DB: Database,
{
    type Output = InterpreterResult;

    fn set_spec(&mut self, _spec: CfgEnv::Spec) -> bool {
        false
    }

    fn run(
        &mut self,
        context: &mut Context<BlockEnv, TxEnv, CfgEnv, DB, Journal<DB>, Chain>,
        address: &Address,
        inputs: &InputsImpl,
        _is_static: bool,
        gas_limit: u64,
    ) -> Result<Option<InterpreterResult>, String> {
        // Get the precompile at the address
        let Some(precompile) = self.get(address) else {
            return Ok(None);
        };

        let mut result = InterpreterResult {
            result: InstructionResult::Return,
            gas: Gas::new(gas_limit),
            output: Bytes::new(),
        };

        let (local, journal) = (&context.local, &mut context.journaled_state);

        // Execute the precompile
        let r;
        let input_bytes = match &inputs.input {
            CallInput::SharedBuffer(range) => {
                // `map_or` does not work here as we use `r` to extend lifetime of the slice
                // and return it.
                #[allow(clippy::option_if_let_else)]
                if let Some(slice) = local.shared_memory_buffer_slice(range.clone()) {
                    r = slice;
                    &*r
                } else {
                    &[]
                }
            }
            CallInput::Bytes(bytes) => bytes.as_ref(),
        };
        let precompile_result = precompile.call(PrecompileInput {
            data: input_bytes,
            gas: gas_limit,
            caller: inputs.caller_address,
            value: inputs.call_value,
            internals: EvmInternals::new(journal, &context.block, &mut context.tx),
            target_address: inputs.target_address,
            bytecode_address: inputs.bytecode_address.expect("always set for precompile calls"),
        });

        match precompile_result {
            Ok(output) => {
                let underflow = result.gas.record_cost(output.gas_used);
                assert!(underflow, "Gas underflow is not possible");
                result.result = if output.reverted {
                    InstructionResult::Revert
                } else {
                    InstructionResult::Return
                };
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

/// A mapping of precompile contracts that can be either static (builtin) or dynamic.
///
/// This is an optimization that allows us to keep using the static precompiles
/// until we need to modify them, at which point we convert to the dynamic representation.
#[derive(Clone)]
enum PrecompilesKind {
    /// Static builtin precompiles.
    Builtin(Cow<'static, Precompiles>),
    /// Dynamic precompiles that can be modified at runtime.
    Dynamic(DynPrecompiles),
}

/// A dynamic precompile implementation that can be modified at runtime.
#[derive(Clone)]
pub struct DynPrecompile(pub(crate) Arc<dyn Precompile + Send + Sync>);

impl DynPrecompile {
    /// Creates a new [`DynPrecompiles`] with the given closure.
    pub fn new<F>(id: PrecompileId, f: F) -> Self
    where
        F: Fn(PrecompileInput<'_>) -> PrecompileResult + Send + Sync + 'static,
    {
        Self(Arc::new((id, f)))
    }

    /// Creates a new [`DynPrecompiles`] with the given closure and [`Precompile::is_pure`]
    /// returning `false`.
    pub fn new_stateful<F>(id: PrecompileId, f: F) -> Self
    where
        F: Fn(PrecompileInput<'_>) -> PrecompileResult + Send + Sync + 'static,
    {
        Self(Arc::new(StatefulPrecompile((id, f))))
    }

    /// Flips [`Precompile::is_pure`] to `false`.
    pub fn stateful(self) -> Self {
        Self(Arc::new(StatefulPrecompile(self.0)))
    }
}

impl core::fmt::Debug for DynPrecompile {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DynPrecompile").finish()
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

impl DynPrecompiles {
    /// Consumes the type and returns an iterator over the addresses and the corresponding
    /// precompile.
    pub fn into_precompiles(self) -> impl Iterator<Item = (Address, DynPrecompile)> {
        self.inner.into_iter()
    }
}

impl core::fmt::Debug for DynPrecompiles {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DynPrecompiles").field("addresses", &self.addresses).finish()
    }
}

/// Input for a precompile call.
#[derive(Debug)]
pub struct PrecompileInput<'a> {
    /// Input data bytes.
    pub data: &'a [u8],
    /// Gas limit.
    pub gas: u64,
    /// Caller address.
    pub caller: Address,
    /// Value sent with the call.
    pub value: U256,
    /// Target address of the call. Would be the same as `bytecode_address` unless it's a
    /// DELEGATECALL.
    pub target_address: Address,
    /// Bytecode address of the call.
    pub bytecode_address: Address,
    /// Various hooks for interacting with the EVM state.
    pub internals: EvmInternals<'a>,
}

impl<'a> PrecompileInput<'a> {
    /// Returns the calldata of the call.
    pub const fn data(&self) -> &[u8] {
        self.data
    }

    /// Returns the caller address of the call.
    pub const fn caller(&self) -> &Address {
        &self.caller
    }

    /// Returns the gas limit of the call.
    pub const fn gas(&self) -> u64 {
        self.gas
    }

    /// Returns the value of the call.
    pub const fn value(&self) -> &U256 {
        &self.value
    }

    /// Returns the target address of the call.
    pub const fn target_address(&self) -> &Address {
        &self.target_address
    }

    /// Returns the bytecode address of the call.
    pub const fn bytecode_address(&self) -> &Address {
        &self.bytecode_address
    }

    /// Returns whether the call is a direct call, i.e when precompile was called directly and not
    /// via a DELEGATECALL/CALLCODE.
    pub fn is_direct_call(&self) -> bool {
        self.target_address == self.bytecode_address
    }

    /// Returns the [`EvmInternals`].
    pub const fn internals(&self) -> &EvmInternals<'_> {
        &self.internals
    }

    /// Returns a mutable reference to the [`EvmInternals`].
    pub const fn internals_mut(&mut self) -> &mut EvmInternals<'a> {
        &mut self.internals
    }
}

/// Trait for implementing precompiled contracts.
#[auto_impl::auto_impl(&, Arc)]
pub trait Precompile {
    /// Returns precompile ID.
    fn precompile_id(&self) -> &PrecompileId;

    /// Execute the precompile with the given input data, gas limit, and caller address.
    fn call(&self, input: PrecompileInput<'_>) -> PrecompileResult;

    /// Returns whether the precompile is pure.
    ///
    /// A pure precompile has deterministic output based solely on its input.
    /// Non-pure precompiles may produce different outputs for the same input
    /// based on the current state or other external factors.
    ///
    /// # Default
    ///
    /// Returns `true` by default, indicating the precompile is pure
    /// and its results should be cached as this is what most of the precompiles are.
    ///
    /// # Examples
    ///
    /// Override this method to return `false` for non-deterministic precompiles:
    ///
    /// ```ignore
    /// impl Precompile for MyDeterministicPrecompile {
    ///     fn call(&self, input: PrecompileInput<'_>) -> PrecompileResult {
    ///         // non-deterministic computation dependent on state
    ///     }
    ///
    ///     fn is_pure(&self) -> bool {
    ///         false // This precompile might produce different output for the same input
    ///     }
    /// }
    /// ```
    fn is_pure(&self) -> bool {
        true
    }
}

impl<F> Precompile for (PrecompileId, F)
where
    F: Fn(PrecompileInput<'_>) -> PrecompileResult + Send + Sync,
{
    fn precompile_id(&self) -> &PrecompileId {
        &self.0
    }

    fn call(&self, input: PrecompileInput<'_>) -> PrecompileResult {
        self.1(input)
    }
}

impl<F> Precompile for (&PrecompileId, F)
where
    F: Fn(PrecompileInput<'_>) -> PrecompileResult + Send + Sync,
{
    fn precompile_id(&self) -> &PrecompileId {
        self.0
    }

    fn call(&self, input: PrecompileInput<'_>) -> PrecompileResult {
        self.1(input)
    }
}

impl Precompile for revm::precompile::Precompile {
    fn precompile_id(&self) -> &PrecompileId {
        self.id()
    }

    fn call(&self, input: PrecompileInput<'_>) -> PrecompileResult {
        self.precompile()(input.data, input.gas)
    }
}

impl<F> From<F> for DynPrecompile
where
    F: Fn(PrecompileInput<'_>) -> PrecompileResult + Send + Sync + 'static,
{
    fn from(f: F) -> Self {
        Self::new(PrecompileId::Custom("closure".into()), f)
    }
}

impl From<PrecompileFn> for DynPrecompile {
    fn from(f: PrecompileFn) -> Self {
        let p = move |input: PrecompileInput<'_>| f(input.data, input.gas);
        p.into()
    }
}

impl<F> From<(PrecompileId, F)> for DynPrecompile
where
    F: Fn(PrecompileInput<'_>) -> PrecompileResult + Send + Sync + 'static,
{
    fn from((id, f): (PrecompileId, F)) -> Self {
        Self(Arc::new((id, f)))
    }
}

impl From<(PrecompileId, PrecompileFn)> for DynPrecompile {
    fn from((id, f): (PrecompileId, PrecompileFn)) -> Self {
        let p = move |input: PrecompileInput<'_>| f(input.data, input.gas);
        (id, p).into()
    }
}

impl Precompile for DynPrecompile {
    fn precompile_id(&self) -> &PrecompileId {
        self.0.precompile_id()
    }

    fn call(&self, input: PrecompileInput<'_>) -> PrecompileResult {
        self.0.call(input)
    }

    fn is_pure(&self) -> bool {
        self.0.is_pure()
    }
}

impl<A: Precompile, B: Precompile> Precompile for Either<A, B> {
    fn precompile_id(&self) -> &PrecompileId {
        match self {
            Self::Left(p) => p.precompile_id(),
            Self::Right(p) => p.precompile_id(),
        }
    }

    fn call(&self, input: PrecompileInput<'_>) -> PrecompileResult {
        match self {
            Self::Left(p) => p.call(input),
            Self::Right(p) => p.call(input),
        }
    }

    fn is_pure(&self) -> bool {
        match self {
            Self::Left(p) => p.is_pure(),
            Self::Right(p) => p.is_pure(),
        }
    }
}

struct StatefulPrecompile<P>(P);

impl<P: Precompile> Precompile for StatefulPrecompile<P> {
    fn precompile_id(&self) -> &PrecompileId {
        self.0.precompile_id()
    }

    fn call(&self, input: PrecompileInput<'_>) -> PrecompileResult {
        self.0.call(input)
    }

    fn is_pure(&self) -> bool {
        false
    }
}

/// Trait for dynamically resolving precompile contracts.
///
/// This trait allows for runtime resolution of precompiles that aren't known
/// at initialization time.
pub trait PrecompileLookup: Send + Sync {
    /// Looks up a precompile at the given address.
    ///
    /// Returns `Some(precompile)` if a precompile exists at the address,
    /// or `None` if no precompile is found.
    fn lookup(&self, address: &Address) -> Option<DynPrecompile>;
}

/// Implement PrecompileLookup for closure types
impl<F> PrecompileLookup for F
where
    F: Fn(&Address) -> Option<DynPrecompile> + Send + Sync,
{
    fn lookup(&self, address: &Address) -> Option<DynPrecompile> {
        self(address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eth::EthEvmContext;
    use alloy_primitives::{address, Bytes};
    use revm::{
        context::Block,
        database::EmptyDB,
        precompile::{PrecompileId, PrecompileOutput},
    };

    #[test]
    fn test_map_precompile() {
        let eth_precompiles = EthPrecompiles::default();
        let mut spec_precompiles = PrecompilesMap::from(eth_precompiles);

        let mut ctx = EthEvmContext::new(EmptyDB::default(), Default::default());

        // create a test input for the precompile (identity precompile)
        let identity_address = address!("0x0000000000000000000000000000000000000004");
        let test_input = Bytes::from_static(b"test data");
        let gas_limit = 1000;

        // Ensure we're using dynamic precompiles
        spec_precompiles.ensure_dynamic_precompiles();

        // using the dynamic precompiles interface
        let dyn_precompile = match &spec_precompiles.precompiles {
            PrecompilesKind::Dynamic(dyn_precompiles) => {
                dyn_precompiles.inner.get(&identity_address).unwrap()
            }
            _ => panic!("Expected dynamic precompiles"),
        };

        let result = dyn_precompile
            .call(PrecompileInput {
                data: &test_input,
                gas: gas_limit,
                caller: Address::ZERO,
                value: U256::ZERO,
                internals: EvmInternals::new(&mut ctx.journaled_state, &ctx.block, &mut ctx.tx),
                target_address: identity_address,
                bytecode_address: identity_address,
            })
            .unwrap();
        assert_eq!(result.bytes, test_input, "Identity precompile should return the input data");

        // define a function to modify the precompile
        // this will change the identity precompile to always return a fixed value
        let constant_bytes = Bytes::from_static(b"constant value");

        // define a function to modify the precompile to always return a constant value
        spec_precompiles.map_precompile(&identity_address, move |_original_dyn| {
            // create a new DynPrecompile that always returns our constant
            |_input: PrecompileInput<'_>| -> PrecompileResult {
                Ok(PrecompileOutput::new(10, Bytes::from_static(b"constant value")))
            }
            .into()
        });

        // get the modified precompile and check it
        let dyn_precompile = match &spec_precompiles.precompiles {
            PrecompilesKind::Dynamic(dyn_precompiles) => {
                dyn_precompiles.inner.get(&identity_address).unwrap()
            }
            _ => panic!("Expected dynamic precompiles"),
        };

        let result = dyn_precompile
            .call(PrecompileInput {
                data: &test_input,
                gas: gas_limit,
                caller: Address::ZERO,
                value: U256::ZERO,
                internals: EvmInternals::new(&mut ctx.journaled_state, &ctx.block, &mut ctx.tx),
                target_address: identity_address,
                bytecode_address: identity_address,
            })
            .unwrap();
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

        let mut ctx = EthEvmContext::new(EmptyDB::default(), Default::default());

        // define a closure that implements the precompile functionality
        let closure_precompile = |input: PrecompileInput<'_>| -> PrecompileResult {
            let _timestamp = input.internals.block_env().timestamp();
            let mut output = b"processed: ".to_vec();
            output.extend_from_slice(input.data.as_ref());
            Ok(PrecompileOutput::new(15, Bytes::from(output)))
        };

        let dyn_precompile: DynPrecompile = closure_precompile.into();

        let result = dyn_precompile
            .call(PrecompileInput {
                data: &test_input,
                gas: gas_limit,
                caller: Address::ZERO,
                value: U256::ZERO,
                internals: EvmInternals::new(&mut ctx.journaled_state, &ctx.block, &mut ctx.tx),
                target_address: Address::ZERO,
                bytecode_address: Address::ZERO,
            })
            .unwrap();
        assert_eq!(result.gas_used, 15);
        assert_eq!(result.bytes, expected_output);
    }

    #[test]
    fn test_is_pure() {
        // Test default behavior (should be false)
        let closure_precompile = |_input: PrecompileInput<'_>| -> PrecompileResult {
            Ok(PrecompileOutput::new(10, Bytes::from_static(b"output")))
        };

        let dyn_precompile: DynPrecompile = closure_precompile.into();
        assert!(dyn_precompile.is_pure(), "should be pure by default");

        // Test custom precompile with overridden is_pure
        let stateful_precompile =
            DynPrecompile::new_stateful(PrecompileId::Custom("closure".into()), closure_precompile);
        assert!(!stateful_precompile.is_pure(), "PurePrecompile should return true for is_pure");

        let either_left = Either::<DynPrecompile, DynPrecompile>::Left(stateful_precompile);
        assert!(!either_left.is_pure(), "Either::Left with non-pure should return false");

        let either_right = Either::<DynPrecompile, DynPrecompile>::Right(dyn_precompile);
        assert!(either_right.is_pure(), "Either::Right with pure should return true");
    }

    #[test]
    fn test_precompile_lookup() {
        let eth_precompiles = EthPrecompiles::default();
        let mut spec_precompiles = PrecompilesMap::from(eth_precompiles);

        let mut ctx = EthEvmContext::new(EmptyDB::default(), Default::default());

        // Define a custom address pattern for dynamic precompiles
        let dynamic_prefix = [0xDE, 0xAD];

        // Set up the lookup function
        spec_precompiles.set_precompile_lookup(move |address: &Address| {
            if address.as_slice().starts_with(&dynamic_prefix) {
                Some(DynPrecompile::new(PrecompileId::Custom("dynamic".into()), |_input| {
                    Ok(PrecompileOutput {
                        gas_used: 100,
                        bytes: Bytes::from("dynamic precompile response"),
                        reverted: false,
                    })
                }))
            } else {
                None
            }
        });

        // Test that static precompiles still work
        let identity_address = address!("0x0000000000000000000000000000000000000004");
        assert!(spec_precompiles.get(&identity_address).is_some());

        // Test dynamic lookup for matching address
        let dynamic_address = address!("0xDEAD000000000000000000000000000000000001");
        let dynamic_precompile = spec_precompiles.get(&dynamic_address);
        assert!(dynamic_precompile.is_some(), "Dynamic precompile should be found");
        // Execute the dynamic precompile
        let result = dynamic_precompile
            .unwrap()
            .call(PrecompileInput {
                data: &[],
                gas: 1000,
                caller: Address::ZERO,
                value: U256::ZERO,
                internals: EvmInternals::new(&mut ctx.journaled_state, &ctx.block, &mut ctx.tx),
                target_address: dynamic_address,
                bytecode_address: dynamic_address,
            })
            .unwrap();
        assert_eq!(result.gas_used, 100);
        assert_eq!(result.bytes, Bytes::from("dynamic precompile response"));

        // Test non-matching address returns None
        let non_matching_address = address!("0x1234000000000000000000000000000000000001");
        assert!(spec_precompiles.get(&non_matching_address).is_none());
    }

    #[test]
    fn test_get_precompile() {
        let eth_precompiles = EthPrecompiles::default();
        let spec_precompiles = PrecompilesMap::from(eth_precompiles);

        let mut ctx = EthEvmContext::new(EmptyDB::default(), Default::default());

        let identity_address = address!("0x0000000000000000000000000000000000000004");
        let test_input = Bytes::from_static(b"test data");
        let gas_limit = 1000;

        let precompile = spec_precompiles.get(&identity_address);
        assert!(precompile.is_some(), "Identity precompile should exist");

        let result = precompile
            .unwrap()
            .call(PrecompileInput {
                data: &test_input,
                gas: gas_limit,
                caller: Address::ZERO,
                value: U256::ZERO,
                target_address: identity_address,
                bytecode_address: identity_address,
                internals: EvmInternals::new(&mut ctx.journaled_state, &ctx.block, &mut ctx.tx),
            })
            .unwrap();
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

        let result = dyn_precompile
            .unwrap()
            .call(PrecompileInput {
                data: &test_input,
                gas: gas_limit,
                caller: Address::ZERO,
                value: U256::ZERO,
                internals: EvmInternals::new(&mut ctx.journaled_state, &ctx.block, &mut ctx.tx),
                target_address: identity_address,
                bytecode_address: identity_address,
            })
            .unwrap();
        assert_eq!(
            result.bytes, test_input,
            "Identity precompile should return the input data after conversion to dynamic"
        );
    }
}
