//! Transaction abstractions for EVM execution.
//!
//! This module provides traits and implementations for converting various transaction formats
//! into a unified transaction environment ([`TxEnv`]) that the EVM can execute. The main purpose
//! of these traits is to enable flexible transaction input while maintaining type safety.

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

/// Trait marking types that can be converted into a transaction environment.
///
/// This is the primary trait that enables flexible transaction input for the EVM. The EVM's
/// associated type `Evm::Tx` must implement this trait, and the `transact` method accepts
/// any type implementing [`IntoTxEnv<Evm::Tx>`](IntoTxEnv).
///
/// # Example
///
/// ```ignore
/// // Direct TxEnv usage
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
    /// Converts `self` into [`TxEnv`].
    fn into_tx_env(self) -> TxEnv;
}

impl IntoTxEnv<Self> for TxEnv {
    fn into_tx_env(self) -> Self {
        self
    }
}

/// A helper trait to allow implementing [`IntoTxEnv`] for types that build transaction environment
/// by cloning data.
#[auto_impl::auto_impl(&)]
pub trait ToTxEnv<TxEnv> {
    /// Builds a [`TxEnv`] from `self`.
    fn to_tx_env(&self) -> TxEnv;
}

impl<T, TxEnv> IntoTxEnv<TxEnv> for T
where
    T: ToTxEnv<TxEnv>,
{
    fn into_tx_env(self) -> TxEnv {
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

/// Container for a split transaction: holds both the EVM transaction environment
/// and the recovered transaction data separately.
///
/// This struct enables zero-copy transaction execution by allowing the `tx_env` to be
/// consumed by the EVM while retaining access to the original transaction data via
/// `recovered` for receipt generation.
///
/// # Example
///
/// ```ignore
/// // Split a transaction into parts
/// let parts: TxParts<TxEnv, Recovered<T>> = tx.into_tx_parts();
///
/// // Execute using the tx_env (zero-copy consumption)
/// let result = evm.transact(parts.tx_env)?;
///
/// // Use recovered for receipt building
/// let receipt = build_receipt(parts.recovered.tx());
/// ```
#[derive(Debug, Clone)]
pub struct TxParts<E, R> {
    /// The transaction environment for EVM execution.
    pub tx_env: E,
    /// The recovered transaction data (provides access to original tx and signer).
    pub recovered: R,
}

impl<E, R> TxParts<E, R> {
    /// Creates a new `TxParts` from its components.
    pub const fn new(tx_env: E, recovered: R) -> Self {
        Self { tx_env, recovered }
    }

    /// Consumes self and returns the inner tx_env.
    pub fn into_tx_env(self) -> E {
        self.tx_env
    }

    /// Returns a reference to the recovered transaction.
    pub const fn recovered(&self) -> &R {
        &self.recovered
    }

    /// Consumes self and returns the recovered transaction.
    pub fn into_recovered(self) -> R {
        self.recovered
    }
}

impl<E, R, Tx> RecoveredTx<Tx> for TxParts<E, R>
where
    R: RecoveredTx<Tx>,
{
    fn tx(&self) -> &Tx {
        self.recovered.tx()
    }

    fn signer(&self) -> &Address {
        self.recovered.signer()
    }
}

impl<E: Clone, R> ToTxEnv<E> for TxParts<E, R> {
    fn to_tx_env(&self) -> E {
        self.tx_env.clone()
    }
}

/// `TxParts` can be split again - just moves the fields.
impl<E, R, Tx> IntoTxParts<E, Tx> for TxParts<E, R>
where
    E: Clone,
    R: RecoveredTx<Tx>,
{
    type Recovered = R;

    fn into_tx_parts(self) -> TxParts<E, Self::Recovered> {
        self
    }
}

/// Trait for types that can be decomposed into [`TxParts`].
///
/// This trait enables zero-copy consumption of the transaction environment while
/// retaining access to the original transaction data for receipt building.
///
/// # Design
///
/// When a type implements this trait, it can be split into:
/// - `TxEnv`: The transaction environment consumed by the EVM
/// - `Recovered`: The remainder that provides [`RecoveredTx`] access
///
/// For types like `WithTxEnv<TxEnv, Arc<T>>` that pre-compute the `TxEnv`,
/// this allows zero-copy extraction of the `TxEnv`.
///
/// # Example
///
/// ```ignore
/// // Split a recovered transaction
/// let parts = recovered_tx.into_tx_parts();
///
/// // parts.tx_env is ready for EVM consumption
/// // parts.recovered still provides tx() and signer() access
/// ```
pub trait IntoTxParts<TxEnv, Tx>: ToTxEnv<TxEnv> + RecoveredTx<Tx> {
    /// The recovered transaction type after splitting.
    type Recovered: RecoveredTx<Tx>;

    /// Splits self into [`TxParts`] containing the transaction environment and recovered data.
    fn into_tx_parts(self) -> TxParts<TxEnv, Self::Recovered>;
}

/// Implementation for references - clones the TxEnv, borrows for RecoveredTx.
impl<'a, T, TxEnv, Tx> IntoTxParts<TxEnv, Tx> for &'a T
where
    T: ToTxEnv<TxEnv> + RecoveredTx<Tx>,
    &'a T: RecoveredTx<Tx>,
{
    type Recovered = &'a T;

    fn into_tx_parts(self) -> TxParts<TxEnv, Self::Recovered> {
        TxParts::new(self.to_tx_env(), self)
    }
}

/// Implementation for `Recovered<T>`.
impl<T, TxEnv> IntoTxParts<TxEnv, T> for Recovered<T>
where
    TxEnv: FromRecoveredTx<T>,
{
    type Recovered = Self;

    fn into_tx_parts(self) -> TxParts<TxEnv, Self::Recovered> {
        let tx_env = TxEnv::from_recovered_tx(self.inner(), self.signer());
        TxParts::new(tx_env, self)
    }
}



/// Implementation for `WithEncoded<Recovered<T>>`.
impl<T, TxEnv> IntoTxParts<TxEnv, T> for WithEncoded<Recovered<T>>
where
    TxEnv: FromTxWithEncoded<T>,
{
    type Recovered = Self;

    fn into_tx_parts(self) -> TxParts<TxEnv, Self::Recovered> {
        let tx_env = TxEnv::from_encoded_tx(
            self.value().inner(),
            self.value().signer(),
            self.encoded_bytes().clone(),
        );
        TxParts::new(tx_env, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
