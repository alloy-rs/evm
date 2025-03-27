//! Abstraction of an executable transaction.

use alloy_consensus::{
    transaction::Recovered, TxEip1559, TxEip2930, TxEip4844, TxEip7702, TxLegacy,
};
use alloy_eips::{eip2718::WithEncoded, Typed2718};
use alloy_primitives::{Address, Bytes, TxKind};
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
            authorization_list: authorization_list.clone(),
            ..Default::default()
        }
    }
}

/// Helper user-facing trait to allow retrieving a recovered transaction reference.
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

impl<T> RecoveredTx<T> for Recovered<T> {
    fn tx(&self) -> &T {
        self.inner()
    }

    fn signer(&self) -> &Address {
        self.signer_ref()
    }
}

impl<T> RecoveredTx<T> for WithEncoded<Recovered<T>> {
    fn tx(&self) -> &T {
        self.1.inner()
    }

    fn signer(&self) -> &Address {
        self.1.signer_ref()
    }
}

/// Helper user-facing trait to allow implementing [`IntoTxEnv`] on instances of [`WithEncoded`].
/// This allows creating transaction environments directly from EIP-2718 encoded bytes.
pub trait FromTxWithEncoded<Tx> {
    /// Builds a `TxEnv` from a transaction, its sender, and encoded transaction bytes.
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

impl<T, TxEnv: FromTxWithEncoded<T>> IntoTxEnv<TxEnv> for WithEncoded<Recovered<T>> {
    fn into_tx_env(self) -> TxEnv {
        let recovered = &self.1;
        TxEnv::from_encoded_tx(recovered.inner(), recovered.signer(), self.encoded_bytes().clone())
    }
}

impl<T, TxEnv: FromTxWithEncoded<T>> IntoTxEnv<TxEnv> for &WithEncoded<Recovered<T>> {
    fn into_tx_env(self) -> TxEnv {
        let recovered = &self.1;
        TxEnv::from_encoded_tx(recovered.inner(), recovered.signer(), self.encoded_bytes().clone())
    }
}

#[cfg(feature = "op")]
mod op {
    use super::*;
    use alloy_eips::{Encodable2718, Typed2718};
    use alloy_primitives::{Address, Bytes};
    use op_alloy_consensus::{OpTxEnvelope, TxDeposit};
    use op_revm::{transaction::deposit::DepositTransactionParts, OpTransaction};
    use revm::context::TxEnv;

    impl FromTxWithEncoded<OpTxEnvelope> for OpTransaction<TxEnv> {
        fn from_encoded_tx(tx: &OpTxEnvelope, caller: Address, encoded: Bytes) -> Self {
            let base = match tx {
                OpTxEnvelope::Legacy(tx) => TxEnv::from_recovered_tx(tx.tx(), caller),
                OpTxEnvelope::Eip1559(tx) => TxEnv::from_recovered_tx(tx.tx(), caller),
                OpTxEnvelope::Eip2930(tx) => TxEnv::from_recovered_tx(tx.tx(), caller),
                OpTxEnvelope::Eip7702(tx) => TxEnv::from_recovered_tx(tx.tx(), caller),
                OpTxEnvelope::Deposit(tx) => {
                    let TxDeposit {
                        to,
                        value,
                        gas_limit,
                        input,
                        source_hash: _,
                        from: _,
                        mint: _,
                        is_system_transaction: _,
                    } = tx.inner();
                    TxEnv {
                        tx_type: tx.ty(),
                        caller,
                        gas_limit: *gas_limit,
                        kind: *to,
                        value: *value,
                        data: input.clone(),
                        ..Default::default()
                    }
                }
            };

            let deposit = if let OpTxEnvelope::Deposit(tx) = tx {
                DepositTransactionParts {
                    source_hash: tx.source_hash,
                    mint: tx.mint,
                    is_system_transaction: tx.is_system_transaction,
                }
            } else {
                Default::default()
            };

            Self { base, enveloped_tx: Some(encoded), deposit }
        }
    }

    impl FromRecoveredTx<OpTxEnvelope> for OpTransaction<TxEnv> {
        fn from_recovered_tx(tx: &OpTxEnvelope, sender: Address) -> Self {
            let encoded = tx.encoded_2718();
            Self::from_encoded_tx(tx, sender, encoded.into())
        }
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
