//! Transaction abstractions for EVM execution.
//!
//! This module provides traits and implementations for converting various transaction formats
//! into a unified transaction environment ([`TxEnv`]) that the EVM can execute. The main purpose
//! of these traits is to enable flexible transaction input while maintaining type safety.
//!
//! # Trait Hierarchy
//!
//! There are two conversion traits:
//!
//! - [`IntoTxEnv`]: Consumes `self` to produce a `TxEnv`. Types that already contain a `TxEnv` (or
//!   can produce one without cloning) should implement this directly for zero-copy consumption.
//!
//! - [`ToTxEnv`]: Builds a `TxEnv` by borrowing from `&self`, typically requiring clones. This is
//!   useful for types that don't own their transaction data.
//!
//! # Blanket Implementations
//!
//! There is a blanket impl `IntoTxEnv<Env> for &T where T: ToTxEnv<Env>` which allows references
//! to types implementing `ToTxEnv` to be used where `IntoTxEnv` is required.
//!
//! However, there is intentionally **no blanket impl** for owned `T: ToTxEnv -> IntoTxEnv`. This
//! allows downstream crates to implement `IntoTxEnv` directly for zero-copy consumption when the
//! type owns its `TxEnv`. For example, a wrapper type like `WithTxEnv<TxEnv, T>` can implement
//! `IntoTxEnv` to extract and return the inner `TxEnv` by value without cloning.
//!
//! # Transaction Environment Reuse
//!
//! For high-throughput scenarios like block execution, this module also provides the [`FillTxEnv`]
//! trait for filling an existing [`TxEnv`] in-place, avoiding per-transaction allocations:
//!
//! ```ignore
//! use alloy_evm::{FillTxEnv, TxEnvExt};
//!
//! let mut tx_env = TxEnv::default();
//!
//! for tx in transactions {
//!     // Fill the existing TxEnv, preserving Vec capacity
//!     tx_env.fill_from_tx(tx.inner(), tx.signer());
//!     let result = evm.transact_raw(&tx_env)?;
//! }
//! ```

use alloc::sync::Arc;
use alloy_consensus::{
    crypto::secp256k1, transaction::Recovered, EthereumTxEnvelope, Signed, TxEip1559, TxEip2930,
    TxEip4844, TxEip4844Variant, TxEip7702, TxLegacy,
};
use alloy_eips::{
    eip2718::WithEncoded,
    eip7702::{RecoveredAuthority, RecoveredAuthorization},
    Typed2718,
};
use alloy_primitives::{Address, Bytes, TxKind};
use revm::{context::TxEnv, context_interface::either::Either};

/// Trait for types that can be consumed to produce a transaction environment.
///
/// This is the primary trait that enables flexible transaction input for the EVM. The EVM's
/// associated type `Evm::Tx` must implement this trait, and the `transact` method accepts
/// any type implementing [`IntoTxEnv<Evm::Tx>`](IntoTxEnv).
///
/// # Zero-Copy Optimization
///
/// Unlike typical `Into`/`From` patterns, there is **no blanket impl** from [`ToTxEnv`] to
/// `IntoTxEnv`. This design allows types that own a `TxEnv` to implement `IntoTxEnv` directly
/// and return it without cloning. For example, a wrapper type like `WithTxEnv<TxEnv, T>` can
/// implement `IntoTxEnv` to extract and return the inner `TxEnv` by value.
///
/// # Example
///
/// ```ignore
/// // Direct TxEnv usage (no conversion needed)
/// let tx_env = TxEnv { caller: address, gas_limit: 100_000, ... };
/// evm.transact(tx_env)?;
///
/// // Using a recovered transaction
/// let recovered = tx.recover_signer()?;
/// evm.transact(recovered)?;
///
/// // Using a transaction with encoded bytes
/// let with_encoded = WithEncoded::new(recovered, encoded_bytes);
/// evm.transact(with_encoded)?;
/// ```
pub trait IntoTxEnv<TxEnv> {
    /// Converts `self` into a [`TxEnv`].
    fn into_tx_env(self) -> TxEnv;
}

impl IntoTxEnv<Self> for TxEnv {
    fn into_tx_env(self) -> Self {
        self
    }
}

/// A helper trait for building a transaction environment by borrowing from `&self`.
///
/// This trait is useful for types that need to clone data to produce a `TxEnv`. It is
/// automatically implemented for references via `#[auto_impl(&)]`.
///
/// # Relationship with IntoTxEnv
///
/// There is a blanket impl `IntoTxEnv for &T where T: ToTxEnv` which allows references to
/// types implementing `ToTxEnv` to be used where `IntoTxEnv` is required. However, there is
/// intentionally **no blanket impl** for owned `T: ToTxEnv`. This allows:
///
/// - References to use `ToTxEnv` (which clones) automatically via `IntoTxEnv`
/// - Owned types to implement `IntoTxEnv` directly for zero-copy consumption
///
/// For owned types that implement `ToTxEnv`, you should also implement `IntoTxEnv`:
///
/// ```ignore
/// impl IntoTxEnv<TxEnv> for MyType {
///     fn into_tx_env(self) -> TxEnv {
///         // Either delegate to to_tx_env() or provide zero-copy implementation
///         self.to_tx_env()
///     }
/// }
/// ```
#[auto_impl::auto_impl(&)]
pub trait ToTxEnv<TxEnv> {
    /// Builds a [`TxEnv`] from `self`.
    fn to_tx_env(&self) -> TxEnv;
}

impl<T, Env> IntoTxEnv<Env> for &T
where
    T: ToTxEnv<Env> + ?Sized,
{
    fn into_tx_env(self) -> Env {
        self.to_tx_env()
    }
}

impl<L, R, TxEnv> ToTxEnv<TxEnv> for Either<L, R>
where
    L: ToTxEnv<TxEnv>,
    R: ToTxEnv<TxEnv>,
{
    fn to_tx_env(&self) -> TxEnv {
        match self {
            Self::Left(l) => l.to_tx_env(),
            Self::Right(r) => r.to_tx_env(),
        }
    }
}

impl<L, R, TxEnv> IntoTxEnv<TxEnv> for Either<L, R>
where
    L: IntoTxEnv<TxEnv>,
    R: IntoTxEnv<TxEnv>,
{
    fn into_tx_env(self) -> TxEnv {
        match self {
            Self::Left(l) => l.into_tx_env(),
            Self::Right(r) => r.into_tx_env(),
        }
    }
}

#[cfg(feature = "op")]
impl<T> IntoTxEnv<Self> for op_revm::OpTransaction<T>
where
    T: revm::context_interface::transaction::Transaction,
{
    fn into_tx_env(self) -> Self {
        self
    }
}

/// Helper trait for building a transaction environment from a recovered transaction.
///
/// This trait enables the conversion of consensus transaction types (which have been recovered
/// with their sender address) into the EVM's transaction environment. It's automatically used
/// when a [`Recovered<T>`] type is passed to the EVM's `transact` method.
///
/// The expectation is that any recovered consensus transaction can be converted into the
/// transaction type that the EVM operates on (typically [`TxEnv`]).
///
/// # Implementation
///
/// This trait is implemented for all standard Ethereum transaction types ([`TxLegacy`],
/// [`TxEip2930`], [`TxEip1559`], [`TxEip4844`], [`TxEip7702`]) and transaction envelopes
/// ([`EthereumTxEnvelope`]).
///
/// # Example
///
/// ```ignore
/// // Recover the signer from a transaction
/// let recovered = tx.recover_signer()?;
///
/// // The recovered transaction can now be used with the EVM
/// // This works because Recovered<T> implements IntoTxEnv when T implements FromRecoveredTx
/// evm.transact(recovered)?;
/// ```
pub trait FromRecoveredTx<Tx> {
    /// Builds a [`TxEnv`] from a transaction and a sender address.
    fn from_recovered_tx(tx: &Tx, sender: Address) -> Self;
}

impl<TxEnv, T> FromRecoveredTx<&T> for TxEnv
where
    TxEnv: FromRecoveredTx<T>,
{
    fn from_recovered_tx(tx: &&T, sender: Address) -> Self {
        TxEnv::from_recovered_tx(tx, sender)
    }
}

impl<T, TxEnv: FromRecoveredTx<T>> ToTxEnv<TxEnv> for Recovered<T> {
    fn to_tx_env(&self) -> TxEnv {
        TxEnv::from_recovered_tx(self.inner(), self.signer())
    }
}

impl<T, TxEnv: FromRecoveredTx<T>> IntoTxEnv<TxEnv> for Recovered<T> {
    fn into_tx_env(self) -> TxEnv {
        self.to_tx_env()
    }
}

impl FromRecoveredTx<TxLegacy> for TxEnv {
    fn from_recovered_tx(tx: &TxLegacy, caller: Address) -> Self {
        let TxLegacy { chain_id, nonce, gas_price, gas_limit, to, value, input } = tx;
        Self {
            tx_type: tx.ty(),
            caller,
            gas_limit: *gas_limit,
            gas_price: *gas_price,
            kind: *to,
            value: *value,
            data: input.clone(),
            nonce: *nonce,
            chain_id: *chain_id,
            ..Default::default()
        }
    }
}

impl FromRecoveredTx<Signed<TxLegacy>> for TxEnv {
    fn from_recovered_tx(tx: &Signed<TxLegacy>, sender: Address) -> Self {
        Self::from_recovered_tx(tx.tx(), sender)
    }
}

impl FromTxWithEncoded<TxLegacy> for TxEnv {
    fn from_encoded_tx(tx: &TxLegacy, sender: Address, _encoded: Bytes) -> Self {
        Self::from_recovered_tx(tx, sender)
    }
}

impl FromRecoveredTx<TxEip2930> for TxEnv {
    fn from_recovered_tx(tx: &TxEip2930, caller: Address) -> Self {
        let TxEip2930 { chain_id, nonce, gas_price, gas_limit, to, value, access_list, input } = tx;
        Self {
            tx_type: tx.ty(),
            caller,
            gas_limit: *gas_limit,
            gas_price: *gas_price,
            kind: *to,
            value: *value,
            data: input.clone(),
            chain_id: Some(*chain_id),
            nonce: *nonce,
            access_list: access_list.clone(),
            ..Default::default()
        }
    }
}

impl FromRecoveredTx<Signed<TxEip2930>> for TxEnv {
    fn from_recovered_tx(tx: &Signed<TxEip2930>, sender: Address) -> Self {
        Self::from_recovered_tx(tx.tx(), sender)
    }
}

impl FromTxWithEncoded<TxEip2930> for TxEnv {
    fn from_encoded_tx(tx: &TxEip2930, sender: Address, _encoded: Bytes) -> Self {
        Self::from_recovered_tx(tx, sender)
    }
}

impl FromRecoveredTx<TxEip1559> for TxEnv {
    fn from_recovered_tx(tx: &TxEip1559, caller: Address) -> Self {
        let TxEip1559 {
            chain_id,
            nonce,
            gas_limit,
            to,
            value,
            input,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            access_list,
        } = tx;
        Self {
            tx_type: tx.ty(),
            caller,
            gas_limit: *gas_limit,
            gas_price: *max_fee_per_gas,
            kind: *to,
            value: *value,
            data: input.clone(),
            nonce: *nonce,
            chain_id: Some(*chain_id),
            gas_priority_fee: Some(*max_priority_fee_per_gas),
            access_list: access_list.clone(),
            ..Default::default()
        }
    }
}

impl FromRecoveredTx<Signed<TxEip1559>> for TxEnv {
    fn from_recovered_tx(tx: &Signed<TxEip1559>, sender: Address) -> Self {
        Self::from_recovered_tx(tx.tx(), sender)
    }
}

impl FromTxWithEncoded<TxEip1559> for TxEnv {
    fn from_encoded_tx(tx: &TxEip1559, sender: Address, _encoded: Bytes) -> Self {
        Self::from_recovered_tx(tx, sender)
    }
}

impl FromRecoveredTx<TxEip4844> for TxEnv {
    fn from_recovered_tx(tx: &TxEip4844, caller: Address) -> Self {
        let TxEip4844 {
            chain_id,
            nonce,
            gas_limit,
            to,
            value,
            input,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            access_list,
            blob_versioned_hashes,
            max_fee_per_blob_gas,
        } = tx;
        Self {
            tx_type: tx.ty(),
            caller,
            gas_limit: *gas_limit,
            gas_price: *max_fee_per_gas,
            kind: TxKind::Call(*to),
            value: *value,
            data: input.clone(),
            nonce: *nonce,
            chain_id: Some(*chain_id),
            gas_priority_fee: Some(*max_priority_fee_per_gas),
            access_list: access_list.clone(),
            blob_hashes: blob_versioned_hashes.clone(),
            max_fee_per_blob_gas: *max_fee_per_blob_gas,
            ..Default::default()
        }
    }
}

impl FromRecoveredTx<Signed<TxEip4844>> for TxEnv {
    fn from_recovered_tx(tx: &Signed<TxEip4844>, sender: Address) -> Self {
        Self::from_recovered_tx(tx.tx(), sender)
    }
}

impl FromTxWithEncoded<TxEip4844> for TxEnv {
    fn from_encoded_tx(tx: &TxEip4844, sender: Address, _encoded: Bytes) -> Self {
        Self::from_recovered_tx(tx, sender)
    }
}

impl<T> FromRecoveredTx<TxEip4844Variant<T>> for TxEnv {
    fn from_recovered_tx(tx: &TxEip4844Variant<T>, sender: Address) -> Self {
        Self::from_recovered_tx(tx.tx(), sender)
    }
}

impl<T> FromRecoveredTx<Signed<TxEip4844Variant<T>>> for TxEnv {
    fn from_recovered_tx(tx: &Signed<TxEip4844Variant<T>>, sender: Address) -> Self {
        Self::from_recovered_tx(tx.tx(), sender)
    }
}

impl<T> FromTxWithEncoded<TxEip4844Variant<T>> for TxEnv {
    fn from_encoded_tx(tx: &TxEip4844Variant<T>, sender: Address, _encoded: Bytes) -> Self {
        Self::from_recovered_tx(tx, sender)
    }
}

impl FromRecoveredTx<TxEip7702> for TxEnv {
    fn from_recovered_tx(tx: &TxEip7702, caller: Address) -> Self {
        let TxEip7702 {
            chain_id,
            nonce,
            gas_limit,
            to,
            value,
            input,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            access_list,
            authorization_list,
        } = tx;
        Self {
            tx_type: tx.ty(),
            caller,
            gas_limit: *gas_limit,
            gas_price: *max_fee_per_gas,
            kind: TxKind::Call(*to),
            value: *value,
            data: input.clone(),
            nonce: *nonce,
            chain_id: Some(*chain_id),
            gas_priority_fee: Some(*max_priority_fee_per_gas),
            access_list: access_list.clone(),
            authorization_list: authorization_list
                .iter()
                .map(|auth| {
                    Either::Right(RecoveredAuthorization::new_unchecked(
                        auth.inner().clone(),
                        auth.signature()
                            .ok()
                            .and_then(|signature| {
                                secp256k1::recover_signer(&signature, auth.signature_hash()).ok()
                            })
                            .map_or(RecoveredAuthority::Invalid, RecoveredAuthority::Valid),
                    ))
                })
                .collect(),
            ..Default::default()
        }
    }
}

impl FromRecoveredTx<Signed<TxEip7702>> for TxEnv {
    fn from_recovered_tx(tx: &Signed<TxEip7702>, sender: Address) -> Self {
        Self::from_recovered_tx(tx.tx(), sender)
    }
}

impl FromTxWithEncoded<TxEip7702> for TxEnv {
    fn from_encoded_tx(tx: &TxEip7702, sender: Address, _encoded: Bytes) -> Self {
        Self::from_recovered_tx(tx, sender)
    }
}

/// Helper trait to abstract over different [`Recovered<T>`] implementations.
///
/// Implemented for [`Recovered<T>`], `Recovered<&T>`, `&Recovered<T>`, `&Recovered<&T>`
#[auto_impl::auto_impl(&)]
pub trait RecoveredTx<T> {
    /// Returns the transaction.
    fn tx(&self) -> &T;

    /// Returns the signer of the transaction.
    fn signer(&self) -> &Address;
}

impl<T> RecoveredTx<T> for Recovered<&T> {
    fn tx(&self) -> &T {
        self.inner()
    }

    fn signer(&self) -> &Address {
        self.signer_ref()
    }
}

impl<T> RecoveredTx<T> for Recovered<Arc<T>> {
    fn tx(&self) -> &T {
        self.inner().as_ref()
    }

    fn signer(&self) -> &Address {
        self.signer_ref()
    }
}

impl<T> RecoveredTx<T> for Recovered<T> {
    fn tx(&self) -> &T {
        self.inner()
    }

    fn signer(&self) -> &Address {
        self.signer_ref()
    }
}

impl<Tx, T: RecoveredTx<Tx>> RecoveredTx<Tx> for WithEncoded<T> {
    fn tx(&self) -> &Tx {
        self.1.tx()
    }

    fn signer(&self) -> &Address {
        self.1.signer()
    }
}

impl<L, R, Tx> RecoveredTx<Tx> for Either<L, R>
where
    L: RecoveredTx<Tx>,
    R: RecoveredTx<Tx>,
{
    fn tx(&self) -> &Tx {
        match self {
            Self::Left(l) => l.tx(),
            Self::Right(r) => r.tx(),
        }
    }

    fn signer(&self) -> &Address {
        match self {
            Self::Left(l) => l.signer(),
            Self::Right(r) => r.signer(),
        }
    }
}

/// Helper trait for building a transaction environment from a transaction with its encoded form.
///
/// This trait enables the conversion of consensus transaction types along with their EIP-2718
/// encoded bytes into the EVM's transaction environment. It's automatically used when a
/// [`WithEncoded<Recovered<T>>`](WithEncoded) type is passed to the EVM's `transact` method.
///
/// The main purpose of this trait is to allow preserving the original encoded transaction data
/// alongside the parsed transaction, which can be useful for:
/// - Signature verification
/// - Transaction hash computation
/// - Re-encoding for network propagation
/// - Optimism transaction handling (which requires encoded data, for Data availability costs).
///
/// # Implementation
///
/// Most implementations simply delegate to [`FromRecoveredTx`], ignoring the encoded bytes.
/// However, specialized implementations (like Optimism's `OpTransaction`) may use the encoded
/// data for additional functionality.
///
/// # Example
///
/// ```ignore
/// // Create a transaction with its encoded form
/// let encoded_bytes = tx.encoded_2718();
/// let recovered = tx.recover_signer()?;
/// let with_encoded = WithEncoded::new(recovered, encoded_bytes);
///
/// // The transaction with encoded data can be used with the EVM
/// evm.transact(with_encoded)?;
/// ```
pub trait FromTxWithEncoded<Tx> {
    /// Builds a [`TxEnv`] from a transaction, its sender, and encoded transaction bytes.
    fn from_encoded_tx(tx: &Tx, sender: Address, encoded: Bytes) -> Self;
}

impl<TxEnv, T> FromTxWithEncoded<&T> for TxEnv
where
    TxEnv: FromTxWithEncoded<T>,
{
    fn from_encoded_tx(tx: &&T, sender: Address, encoded: Bytes) -> Self {
        TxEnv::from_encoded_tx(tx, sender, encoded)
    }
}

impl<T, TxEnv: FromTxWithEncoded<T>> ToTxEnv<TxEnv> for WithEncoded<Recovered<T>> {
    fn to_tx_env(&self) -> TxEnv {
        let recovered = &self.1;
        TxEnv::from_encoded_tx(recovered.inner(), recovered.signer(), self.encoded_bytes().clone())
    }
}

impl<T, TxEnv: FromTxWithEncoded<T>> IntoTxEnv<TxEnv> for WithEncoded<Recovered<T>> {
    fn into_tx_env(self) -> TxEnv {
        self.to_tx_env()
    }
}

impl<T, TxEnv: FromTxWithEncoded<T>> ToTxEnv<TxEnv> for WithEncoded<&Recovered<T>> {
    fn to_tx_env(&self) -> TxEnv {
        TxEnv::from_encoded_tx(self.value(), *self.value().signer(), self.encoded_bytes().clone())
    }
}

impl<Eip4844: AsRef<TxEip4844>> FromTxWithEncoded<EthereumTxEnvelope<Eip4844>> for TxEnv {
    fn from_encoded_tx(tx: &EthereumTxEnvelope<Eip4844>, caller: Address, encoded: Bytes) -> Self {
        match tx {
            EthereumTxEnvelope::Legacy(tx) => Self::from_encoded_tx(tx.tx(), caller, encoded),
            EthereumTxEnvelope::Eip1559(tx) => Self::from_encoded_tx(tx.tx(), caller, encoded),
            EthereumTxEnvelope::Eip2930(tx) => Self::from_encoded_tx(tx.tx(), caller, encoded),
            EthereumTxEnvelope::Eip4844(tx) => {
                Self::from_encoded_tx(tx.tx().as_ref(), caller, encoded)
            }
            EthereumTxEnvelope::Eip7702(tx) => Self::from_encoded_tx(tx.tx(), caller, encoded),
        }
    }
}

impl<Eip4844: AsRef<TxEip4844>> FromRecoveredTx<EthereumTxEnvelope<Eip4844>> for TxEnv {
    fn from_recovered_tx(tx: &EthereumTxEnvelope<Eip4844>, sender: Address) -> Self {
        match tx {
            EthereumTxEnvelope::Legacy(tx) => Self::from_recovered_tx(tx.tx(), sender),
            EthereumTxEnvelope::Eip1559(tx) => Self::from_recovered_tx(tx.tx(), sender),
            EthereumTxEnvelope::Eip2930(tx) => Self::from_recovered_tx(tx.tx(), sender),
            EthereumTxEnvelope::Eip4844(tx) => Self::from_recovered_tx(tx.tx().as_ref(), sender),
            EthereumTxEnvelope::Eip7702(tx) => Self::from_recovered_tx(tx.tx(), sender),
        }
    }
}

// ================================================================================================
// FillTxEnv - In-place transaction environment filling for allocation reuse
// ================================================================================================

/// A trait for filling an existing [`TxEnv`] in-place, avoiding allocation.
///
/// This is the in-place counterpart to [`FromRecoveredTx`]. Instead of creating a new `TxEnv`,
/// it clears and fills an existing one while preserving the capacity of heap-allocated fields
/// like `access_list`, `blob_hashes`, and `authorization_list`.
///
/// # Safety Guarantees
///
/// Implementations **must** completely reset all fields to valid values for the transaction type.
/// This includes explicitly setting fields to their "empty" values (like `None`, `0`, or empty
/// `Vec`s) even if they were cleared, to prevent stale data from leaking between transactions.
///
/// # Example
///
/// ```ignore
/// use alloy_evm::FillTxEnv;
/// use revm::context::TxEnv;
///
/// let mut tx_env = TxEnv::default();
///
/// // Fill with a legacy transaction
/// tx_env.fill_from_tx(&legacy_tx, sender);
///
/// // Later, fill with an EIP-4844 transaction - all legacy fields are properly reset
/// tx_env.fill_from_tx(&eip4844_tx, sender);
/// ```
pub trait FillTxEnv<Tx> {
    /// Fills `self` with data from the transaction and sender address.
    ///
    /// This method completely resets all fields to match the given transaction,
    /// preserving only the allocated capacity of Vec fields.
    fn fill_from_tx(&mut self, tx: &Tx, sender: Address);
}

/// Extension trait for [`TxEnv`] providing utilities for in-place reuse.
pub trait TxEnvExt {
    /// Clears the transaction environment while preserving allocated capacity.
    ///
    /// This method clears all Vec fields (`access_list`, `blob_hashes`, `authorization_list`)
    /// using `clear()` to preserve their capacity, and resets `data` to empty bytes.
    ///
    /// Note: This only clears heap-allocated fields. Use [`FillTxEnv::fill_from_tx`] to
    /// properly set all fields for a new transaction.
    fn clear_for_reuse(&mut self);
}

impl TxEnvExt for TxEnv {
    fn clear_for_reuse(&mut self) {
        self.access_list.0.clear();
        self.blob_hashes.clear();
        self.authorization_list.clear();
        self.data = Bytes::new();
    }
}

impl FillTxEnv<TxLegacy> for TxEnv {
    fn fill_from_tx(&mut self, tx: &TxLegacy, caller: Address) {
        self.clear_for_reuse();

        self.tx_type = tx.ty();
        self.caller = caller;
        self.gas_limit = tx.gas_limit;
        self.gas_price = tx.gas_price;
        self.kind = tx.to;
        self.value = tx.value;
        self.data = tx.input.clone();
        self.nonce = tx.nonce;
        self.chain_id = tx.chain_id;
        self.gas_priority_fee = None;
        self.max_fee_per_blob_gas = 0;
    }
}

impl FillTxEnv<Signed<TxLegacy>> for TxEnv {
    fn fill_from_tx(&mut self, tx: &Signed<TxLegacy>, sender: Address) {
        self.fill_from_tx(tx.tx(), sender);
    }
}

impl FillTxEnv<TxEip2930> for TxEnv {
    fn fill_from_tx(&mut self, tx: &TxEip2930, caller: Address) {
        self.clear_for_reuse();

        self.tx_type = tx.ty();
        self.caller = caller;
        self.gas_limit = tx.gas_limit;
        self.gas_price = tx.gas_price;
        self.kind = tx.to;
        self.value = tx.value;
        self.data = tx.input.clone();
        self.nonce = tx.nonce;
        self.chain_id = Some(tx.chain_id);
        self.gas_priority_fee = None;
        self.max_fee_per_blob_gas = 0;

        self.access_list = tx.access_list.clone();
    }
}

impl FillTxEnv<Signed<TxEip2930>> for TxEnv {
    fn fill_from_tx(&mut self, tx: &Signed<TxEip2930>, sender: Address) {
        self.fill_from_tx(tx.tx(), sender);
    }
}

impl FillTxEnv<TxEip1559> for TxEnv {
    fn fill_from_tx(&mut self, tx: &TxEip1559, caller: Address) {
        self.clear_for_reuse();

        self.tx_type = tx.ty();
        self.caller = caller;
        self.gas_limit = tx.gas_limit;
        self.gas_price = tx.max_fee_per_gas;
        self.kind = tx.to;
        self.value = tx.value;
        self.data = tx.input.clone();
        self.nonce = tx.nonce;
        self.chain_id = Some(tx.chain_id);
        self.gas_priority_fee = Some(tx.max_priority_fee_per_gas);
        self.max_fee_per_blob_gas = 0;

        self.access_list = tx.access_list.clone();
    }
}

impl FillTxEnv<Signed<TxEip1559>> for TxEnv {
    fn fill_from_tx(&mut self, tx: &Signed<TxEip1559>, sender: Address) {
        self.fill_from_tx(tx.tx(), sender);
    }
}

impl FillTxEnv<TxEip4844> for TxEnv {
    fn fill_from_tx(&mut self, tx: &TxEip4844, caller: Address) {
        self.clear_for_reuse();

        self.tx_type = tx.ty();
        self.caller = caller;
        self.gas_limit = tx.gas_limit;
        self.gas_price = tx.max_fee_per_gas;
        self.kind = TxKind::Call(tx.to);
        self.value = tx.value;
        self.data = tx.input.clone();
        self.nonce = tx.nonce;
        self.chain_id = Some(tx.chain_id);
        self.gas_priority_fee = Some(tx.max_priority_fee_per_gas);
        self.max_fee_per_blob_gas = tx.max_fee_per_blob_gas;

        self.access_list = tx.access_list.clone();
        self.blob_hashes.extend_from_slice(&tx.blob_versioned_hashes);
    }
}

impl<T: AsRef<TxEip4844>> FillTxEnv<TxEip4844Variant<T>> for TxEnv {
    fn fill_from_tx(&mut self, tx: &TxEip4844Variant<T>, caller: Address) {
        self.fill_from_tx(tx.tx().as_ref(), caller);
    }
}

impl<T: AsRef<TxEip4844>> FillTxEnv<Signed<TxEip4844Variant<T>>> for TxEnv {
    fn fill_from_tx(&mut self, tx: &Signed<TxEip4844Variant<T>>, sender: Address) {
        self.fill_from_tx(tx.tx(), sender);
    }
}

impl FillTxEnv<TxEip7702> for TxEnv {
    fn fill_from_tx(&mut self, tx: &TxEip7702, caller: Address) {
        self.clear_for_reuse();

        self.tx_type = tx.ty();
        self.caller = caller;
        self.gas_limit = tx.gas_limit;
        self.gas_price = tx.max_fee_per_gas;
        self.kind = TxKind::Call(tx.to);
        self.value = tx.value;
        self.data = tx.input.clone();
        self.nonce = tx.nonce;
        self.chain_id = Some(tx.chain_id);
        self.gas_priority_fee = Some(tx.max_priority_fee_per_gas);
        self.max_fee_per_blob_gas = 0;

        self.access_list = tx.access_list.clone();

        self.authorization_list.extend(tx.authorization_list.iter().map(|auth| {
            Either::Right(RecoveredAuthorization::new_unchecked(
                auth.inner().clone(),
                auth.signature()
                    .ok()
                    .and_then(|signature| {
                        secp256k1::recover_signer(&signature, auth.signature_hash()).ok()
                    })
                    .map_or(RecoveredAuthority::Invalid, RecoveredAuthority::Valid),
            ))
        }));
    }
}

impl FillTxEnv<Signed<TxEip7702>> for TxEnv {
    fn fill_from_tx(&mut self, tx: &Signed<TxEip7702>, sender: Address) {
        self.fill_from_tx(tx.tx(), sender);
    }
}

impl<Eip4844: AsRef<TxEip4844>> FillTxEnv<EthereumTxEnvelope<Eip4844>> for TxEnv {
    fn fill_from_tx(&mut self, tx: &EthereumTxEnvelope<Eip4844>, caller: Address) {
        match tx {
            EthereumTxEnvelope::Legacy(tx) => self.fill_from_tx(tx.tx(), caller),
            EthereumTxEnvelope::Eip1559(tx) => self.fill_from_tx(tx.tx(), caller),
            EthereumTxEnvelope::Eip2930(tx) => self.fill_from_tx(tx.tx(), caller),
            EthereumTxEnvelope::Eip4844(tx) => self.fill_from_tx(tx.tx().as_ref(), caller),
            EthereumTxEnvelope::Eip7702(tx) => self.fill_from_tx(tx.tx(), caller),
        }
    }
}

impl<T, TxEnvT> FillTxEnv<Recovered<T>> for TxEnvT
where
    TxEnvT: FillTxEnv<T>,
{
    fn fill_from_tx(&mut self, recovered: &Recovered<T>, _sender: Address) {
        self.fill_from_tx(recovered.inner(), recovered.signer());
    }
}

impl<T, TxEnvT> FillTxEnv<WithEncoded<Recovered<T>>> for TxEnvT
where
    TxEnvT: FillTxEnv<T>,
{
    fn fill_from_tx(&mut self, with_encoded: &WithEncoded<Recovered<T>>, _sender: Address) {
        let recovered = &with_encoded.1;
        self.fill_from_tx(recovered.inner(), recovered.signer());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{address, b256, U256};

    struct MyTxEnv;
    struct MyTransaction;

    impl IntoTxEnv<Self> for MyTxEnv {
        fn into_tx_env(self) -> Self {
            self
        }
    }

    impl FromRecoveredTx<MyTransaction> for MyTxEnv {
        fn from_recovered_tx(_tx: &MyTransaction, _sender: Address) -> Self {
            Self
        }
    }

    impl FromTxWithEncoded<MyTransaction> for MyTxEnv {
        fn from_encoded_tx(_tx: &MyTransaction, _sender: Address, _encoded: Bytes) -> Self {
            Self
        }
    }

    const fn assert_env<T: IntoTxEnv<MyTxEnv>>() {}
    const fn assert_recoverable<T: RecoveredTx<MyTransaction>>() {}

    #[test]
    const fn test_into_tx_env() {
        assert_env::<MyTxEnv>();
        assert_env::<&Recovered<MyTransaction>>();
        assert_env::<&Recovered<&MyTransaction>>();
    }

    #[test]
    const fn test_into_encoded_tx_env() {
        assert_env::<WithEncoded<Recovered<MyTransaction>>>();
        assert_env::<&WithEncoded<Recovered<MyTransaction>>>();

        assert_recoverable::<Recovered<MyTransaction>>();
        assert_recoverable::<WithEncoded<Recovered<MyTransaction>>>();
    }

    // ========================================================================================
    // FillTxEnv tests - comprehensive coverage of in-place filling
    // ========================================================================================

    fn test_sender() -> Address {
        address!("0xabcdefabcdefabcdefabcdefabcdefabcdefabcd")
    }

    fn test_target() -> Address {
        address!("0x1234567890123456789012345678901234567890")
    }

    fn make_legacy_tx() -> TxLegacy {
        TxLegacy {
            chain_id: Some(1),
            nonce: 42,
            gas_price: 100,
            gas_limit: 21000,
            to: TxKind::Call(test_target()),
            value: U256::from(1000),
            input: Bytes::from_static(b"legacy data"),
        }
    }

    fn make_eip2930_tx() -> TxEip2930 {
        use revm::context_interface::transaction::AccessListItem;
        TxEip2930 {
            chain_id: 2,
            nonce: 43,
            gas_price: 200,
            gas_limit: 30000,
            to: TxKind::Call(test_target()),
            value: U256::from(2000),
            input: Bytes::from_static(b"eip2930 data"),
            access_list: revm::context_interface::transaction::AccessList(vec![AccessListItem {
                address: address!("0x1111111111111111111111111111111111111111"),
                storage_keys: vec![b256!(
                    "0x0000000000000000000000000000000000000000000000000000000000000001"
                )],
            }]),
        }
    }

    fn make_eip1559_tx() -> TxEip1559 {
        use revm::context_interface::transaction::AccessListItem;
        TxEip1559 {
            chain_id: 3,
            nonce: 44,
            max_fee_per_gas: 300,
            max_priority_fee_per_gas: 50,
            gas_limit: 40000,
            to: TxKind::Call(test_target()),
            value: U256::from(3000),
            input: Bytes::from_static(b"eip1559 data"),
            access_list: revm::context_interface::transaction::AccessList(vec![AccessListItem {
                address: address!("0x2222222222222222222222222222222222222222"),
                storage_keys: vec![],
            }]),
        }
    }

    fn make_eip4844_tx() -> TxEip4844 {
        use revm::context_interface::transaction::AccessListItem;
        TxEip4844 {
            chain_id: 4,
            nonce: 45,
            max_fee_per_gas: 400,
            max_priority_fee_per_gas: 60,
            gas_limit: 50000,
            to: test_target(),
            value: U256::from(4000),
            input: Bytes::from_static(b"eip4844 data"),
            access_list: revm::context_interface::transaction::AccessList(vec![AccessListItem {
                address: address!("0x3333333333333333333333333333333333333333"),
                storage_keys: vec![
                    b256!("0x0000000000000000000000000000000000000000000000000000000000000002"),
                    b256!("0x0000000000000000000000000000000000000000000000000000000000000003"),
                ],
            }]),
            blob_versioned_hashes: vec![
                b256!("0x0102030405060708091011121314151617181920212223242526272829303132"),
                b256!("0x0102030405060708091011121314151617181920212223242526272829303133"),
            ],
            max_fee_per_blob_gas: 500,
        }
    }

    fn make_eip7702_tx() -> TxEip7702 {
        use alloy_eips::eip7702::Authorization;
        use revm::context_interface::transaction::AccessListItem;
        TxEip7702 {
            chain_id: 5,
            nonce: 46,
            max_fee_per_gas: 500,
            max_priority_fee_per_gas: 70,
            gas_limit: 60000,
            to: test_target(),
            value: U256::from(5000),
            input: Bytes::from_static(b"eip7702 data"),
            access_list: revm::context_interface::transaction::AccessList(vec![AccessListItem {
                address: address!("0x4444444444444444444444444444444444444444"),
                storage_keys: vec![],
            }]),
            authorization_list: vec![Authorization {
                chain_id: U256::from(1),
                address: address!("0x5555555555555555555555555555555555555555"),
                nonce: 0,
            }
            .into_signed(alloy_primitives::Signature::test_signature())],
        }
    }

    #[test]
    fn test_fill_legacy_tx() {
        let mut tx_env = TxEnv::default();
        let tx = make_legacy_tx();
        let sender = test_sender();

        tx_env.fill_from_tx(&tx, sender);

        assert_eq!(tx_env.tx_type, 0);
        assert_eq!(tx_env.caller, sender);
        assert_eq!(tx_env.nonce, 42);
        assert_eq!(tx_env.gas_price, 100);
        assert_eq!(tx_env.gas_limit, 21000);
        assert_eq!(tx_env.value, U256::from(1000));
        assert_eq!(tx_env.chain_id, Some(1));
        assert_eq!(tx_env.gas_priority_fee, None);
        assert_eq!(tx_env.max_fee_per_blob_gas, 0);
        assert!(tx_env.access_list.0.is_empty());
        assert!(tx_env.blob_hashes.is_empty());
        assert!(tx_env.authorization_list.is_empty());
    }

    #[test]
    fn test_fill_eip4844_tx() {
        let mut tx_env = TxEnv::default();
        let tx = make_eip4844_tx();
        let sender = test_sender();

        tx_env.fill_from_tx(&tx, sender);

        assert_eq!(tx_env.tx_type, 3);
        assert_eq!(tx_env.nonce, 45);
        assert_eq!(tx_env.gas_priority_fee, Some(60));
        assert_eq!(tx_env.max_fee_per_blob_gas, 500);
        assert_eq!(tx_env.blob_hashes.len(), 2);
        assert_eq!(tx_env.access_list.0.len(), 1);
        assert!(tx_env.authorization_list.is_empty());
    }

    #[test]
    fn test_fill_eip7702_tx() {
        let mut tx_env = TxEnv::default();
        let tx = make_eip7702_tx();
        let sender = test_sender();

        tx_env.fill_from_tx(&tx, sender);

        assert_eq!(tx_env.tx_type, 4);
        assert_eq!(tx_env.nonce, 46);
        assert_eq!(tx_env.gas_priority_fee, Some(70));
        assert_eq!(tx_env.max_fee_per_blob_gas, 0);
        assert!(tx_env.blob_hashes.is_empty());
        assert_eq!(tx_env.authorization_list.len(), 1);
    }

    #[test]
    fn test_clear_preserves_capacity() {
        let mut tx_env = TxEnv::default();

        tx_env
            .blob_hashes
            .push(b256!("0x0102030405060708091011121314151617181920212223242526272829303132"));
        tx_env
            .blob_hashes
            .push(b256!("0x0102030405060708091011121314151617181920212223242526272829303133"));
        let capacity_before = tx_env.blob_hashes.capacity();

        tx_env.clear_for_reuse();

        assert!(tx_env.blob_hashes.is_empty());
        assert_eq!(tx_env.blob_hashes.capacity(), capacity_before);
    }

    // ========================================================================================
    // Critical: Test all tx type transitions to ensure no stale data leaks
    // ========================================================================================

    #[test]
    fn test_transition_eip4844_to_legacy_clears_blobs() {
        let mut tx_env = TxEnv::default();
        let sender = test_sender();

        // First fill with EIP-4844 (has blob_hashes and max_fee_per_blob_gas)
        tx_env.fill_from_tx(&make_eip4844_tx(), sender);
        assert_eq!(tx_env.blob_hashes.len(), 2);
        assert_eq!(tx_env.max_fee_per_blob_gas, 500);

        // Now fill with Legacy - blob fields must be cleared
        tx_env.fill_from_tx(&make_legacy_tx(), sender);
        assert!(tx_env.blob_hashes.is_empty(), "blob_hashes not cleared after Legacy fill");
        assert_eq!(tx_env.max_fee_per_blob_gas, 0, "max_fee_per_blob_gas not reset");
        assert_eq!(tx_env.gas_priority_fee, None, "gas_priority_fee not reset to None");
    }

    #[test]
    fn test_transition_eip7702_to_legacy_clears_authorizations() {
        let mut tx_env = TxEnv::default();
        let sender = test_sender();

        // First fill with EIP-7702 (has authorization_list)
        tx_env.fill_from_tx(&make_eip7702_tx(), sender);
        assert_eq!(tx_env.authorization_list.len(), 1);

        // Now fill with Legacy - authorization_list must be cleared
        tx_env.fill_from_tx(&make_legacy_tx(), sender);
        assert!(
            tx_env.authorization_list.is_empty(),
            "authorization_list not cleared after Legacy fill"
        );
    }

    #[test]
    fn test_transition_eip2930_to_legacy_clears_access_list() {
        let mut tx_env = TxEnv::default();
        let sender = test_sender();

        // First fill with EIP-2930 (has access_list)
        tx_env.fill_from_tx(&make_eip2930_tx(), sender);
        assert_eq!(tx_env.access_list.0.len(), 1);

        // Now fill with Legacy - access_list must be cleared
        tx_env.fill_from_tx(&make_legacy_tx(), sender);
        assert!(tx_env.access_list.0.is_empty(), "access_list not cleared after Legacy fill");
    }

    #[test]
    fn test_transition_eip1559_to_eip2930_resets_priority_fee() {
        let mut tx_env = TxEnv::default();
        let sender = test_sender();

        // First fill with EIP-1559 (has gas_priority_fee)
        tx_env.fill_from_tx(&make_eip1559_tx(), sender);
        assert_eq!(tx_env.gas_priority_fee, Some(50));

        // Now fill with EIP-2930 - gas_priority_fee must be None
        tx_env.fill_from_tx(&make_eip2930_tx(), sender);
        assert_eq!(
            tx_env.gas_priority_fee, None,
            "gas_priority_fee not reset to None after EIP-2930 fill"
        );
    }

    #[test]
    fn test_transition_eip4844_to_eip7702() {
        let mut tx_env = TxEnv::default();
        let sender = test_sender();

        // Fill with EIP-4844 (blobs, no auth)
        tx_env.fill_from_tx(&make_eip4844_tx(), sender);
        assert_eq!(tx_env.blob_hashes.len(), 2);
        assert!(tx_env.authorization_list.is_empty());

        // Fill with EIP-7702 (auth, no blobs)
        tx_env.fill_from_tx(&make_eip7702_tx(), sender);
        assert!(tx_env.blob_hashes.is_empty(), "blob_hashes not cleared after EIP-7702 fill");
        assert_eq!(tx_env.max_fee_per_blob_gas, 0, "max_fee_per_blob_gas not reset");
        assert_eq!(tx_env.authorization_list.len(), 1);
    }

    #[test]
    fn test_transition_eip7702_to_eip4844() {
        let mut tx_env = TxEnv::default();
        let sender = test_sender();

        // Fill with EIP-7702 (auth, no blobs)
        tx_env.fill_from_tx(&make_eip7702_tx(), sender);
        assert_eq!(tx_env.authorization_list.len(), 1);
        assert!(tx_env.blob_hashes.is_empty());

        // Fill with EIP-4844 (blobs, no auth)
        tx_env.fill_from_tx(&make_eip4844_tx(), sender);
        assert!(
            tx_env.authorization_list.is_empty(),
            "authorization_list not cleared after EIP-4844 fill"
        );
        assert_eq!(tx_env.blob_hashes.len(), 2);
    }

    #[test]
    fn test_all_fields_reset_on_legacy_fill() {
        let mut tx_env = TxEnv::default();
        let sender = test_sender();

        // Populate all optional fields with non-default values
        tx_env.fill_from_tx(&make_eip4844_tx(), sender);
        tx_env.fill_from_tx(&make_eip7702_tx(), sender);
        // Now tx_env has auth_list from 7702, but blobs were cleared

        // Re-add blobs manually to simulate worst case
        tx_env
            .blob_hashes
            .push(b256!("0x0102030405060708091011121314151617181920212223242526272829303132"));

        // Fill with Legacy - everything must be properly reset
        tx_env.fill_from_tx(&make_legacy_tx(), sender);

        assert_eq!(tx_env.tx_type, 0);
        assert_eq!(tx_env.gas_priority_fee, None);
        assert_eq!(tx_env.max_fee_per_blob_gas, 0);
        assert!(tx_env.access_list.0.is_empty());
        assert!(tx_env.blob_hashes.is_empty());
        assert!(tx_env.authorization_list.is_empty());
    }

    #[test]
    fn test_reuse_multiple_times() {
        let mut tx_env = TxEnv::default();
        let sender = test_sender();

        // Cycle through all tx types multiple times
        for _ in 0..3 {
            tx_env.fill_from_tx(&make_legacy_tx(), sender);
            assert_eq!(tx_env.tx_type, 0);

            tx_env.fill_from_tx(&make_eip2930_tx(), sender);
            assert_eq!(tx_env.tx_type, 1);

            tx_env.fill_from_tx(&make_eip1559_tx(), sender);
            assert_eq!(tx_env.tx_type, 2);

            tx_env.fill_from_tx(&make_eip4844_tx(), sender);
            assert_eq!(tx_env.tx_type, 3);

            tx_env.fill_from_tx(&make_eip7702_tx(), sender);
            assert_eq!(tx_env.tx_type, 4);
        }

        // Final check - should be in valid EIP-7702 state
        assert_eq!(tx_env.authorization_list.len(), 1);
        assert!(tx_env.blob_hashes.is_empty());
    }
}
