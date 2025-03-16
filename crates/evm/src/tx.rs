//! Abstraction of an executable transaction.

use alloy_consensus::transaction::Recovered;
use alloy_eips::eip2718::WithEncoded;
use alloy_primitives::Address;
use revm::context::TxEnv;

/// Trait marking types that can be converted into a transaction environment.
pub trait IntoTxEnv<TxEnv> {
    /// Converts `self` into [`TxEnv`].
    fn into_tx_env(self) -> TxEnv;
}

impl IntoTxEnv<Self> for TxEnv {
    fn into_tx_env(self) -> Self {
        self
    }
}

#[cfg(feature = "op")]
impl<T: revm::context::Transaction> IntoTxEnv<Self> for op_revm::OpTransaction<T> {
    fn into_tx_env(self) -> Self {
        self
    }
}

/// Helper user-facing trait to allow implementing [`IntoTxEnv`] on instances of [`Recovered`].
pub trait FromRecoveredTx<Tx> {
    /// Builds a `TxEnv` from a transaction and a sender address.
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

impl<T, TxEnv: FromRecoveredTx<T>> IntoTxEnv<TxEnv> for Recovered<T> {
    fn into_tx_env(self) -> TxEnv {
        IntoTxEnv::into_tx_env(&self)
    }
}

impl<T, TxEnv: FromRecoveredTx<T>> IntoTxEnv<TxEnv> for &Recovered<T> {
    fn into_tx_env(self) -> TxEnv {
        TxEnv::from_recovered_tx(self.inner(), self.signer())
    }
}

/// Helper user-facing trait to allow retrieving a recovered transaction reference.
pub trait AsRecoveredTx<T> {
    /// Returns a reference to the recovered transaction.
    fn as_recovered(&self) -> Recovered<&T>;
}

impl<T> AsRecoveredTx<T> for Recovered<&T> {
    fn as_recovered(&self) -> Recovered<&T> {
        Recovered::new_unchecked(self.inner(), self.signer())
    }
}

/// Helper user-facing trait to allow implementing [`IntoTxEnv`] on instances of [`WithEncoded`].
/// This allows creating transaction environments directly from EIP-2718 encoded bytes.
pub trait FromEncodedTx<Tx> {
    /// Builds a `TxEnv` from encoded transaction bytes.
    fn from_encoded_tx(encoded: &[u8]) -> Self;
}

impl<TxEnv, T> FromEncodedTx<&T> for TxEnv
where
    TxEnv: FromEncodedTx<T>,
{
    fn from_encoded_tx(encoded: &[u8]) -> Self {
        TxEnv::from_encoded_tx(encoded)
    }
}

impl<T, TxEnv: FromEncodedTx<T>> IntoTxEnv<TxEnv> for WithEncoded<T> {
    fn into_tx_env(self) -> TxEnv {
        TxEnv::from_encoded_tx(self.encoded_bytes())
    }
}

impl<T, TxEnv: FromEncodedTx<T>> IntoTxEnv<TxEnv> for &WithEncoded<T> {
    fn into_tx_env(self) -> TxEnv {
        TxEnv::from_encoded_tx(self.encoded_bytes())
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

    impl FromEncodedTx<MyTransaction> for MyTxEnv {
        fn from_encoded_tx(_encoded: &[u8]) -> Self {
            Self
        }
    }

    const fn assert_env<T: IntoTxEnv<MyTxEnv>>() {}

    #[test]
    const fn test_into_tx_env() {
        assert_env::<MyTxEnv>();
        assert_env::<&Recovered<MyTransaction>>();
        assert_env::<&Recovered<&MyTransaction>>();
    }

    #[test]
    const fn test_into_encoded_tx_env() {
        assert_env::<WithEncoded<MyTransaction>>();
        assert_env::<&WithEncoded<MyTransaction>>();
        assert_env::<WithEncoded<&MyTransaction>>();
        assert_env::<&WithEncoded<&MyTransaction>>();
    }
}
