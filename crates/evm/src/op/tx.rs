use crate::{FromRecoveredTx, FromTxWithEncoded};

use alloy_consensus::{Signed, TxEip1559, TxEip2930, TxEip4844, TxEip7702, TxLegacy};
use alloy_eips::{Encodable2718, Typed2718};
use alloy_primitives::{Address, Bytes};
use op_alloy::consensus::{OpTxEnvelope, TxDeposit};
use op_revm::{transaction::deposit::DepositTransactionParts, OpTransaction};
use revm::context::TxEnv;

impl FromRecoveredTx<OpTxEnvelope> for TxEnv {
    fn from_recovered_tx(tx: &OpTxEnvelope, caller: Address) -> Self {
        match tx {
            OpTxEnvelope::Legacy(tx) => Self::from_recovered_tx(tx.tx(), caller),
            OpTxEnvelope::Eip1559(tx) => Self::from_recovered_tx(tx.tx(), caller),
            OpTxEnvelope::Eip2930(tx) => Self::from_recovered_tx(tx.tx(), caller),
            OpTxEnvelope::Eip7702(tx) => Self::from_recovered_tx(tx.tx(), caller),
            OpTxEnvelope::Deposit(tx) => Self::from_recovered_tx(tx.inner(), caller),
        }
    }
}

impl FromRecoveredTx<TxDeposit> for TxEnv {
    fn from_recovered_tx(tx: &TxDeposit, caller: Address) -> Self {
        let TxDeposit {
            to,
            value,
            gas_limit,
            input,
            source_hash: _,
            from: _,
            mint: _,
            is_system_transaction: _,
        } = tx;
        Self {
            tx_type: tx.ty(),
            caller,
            gas_limit: *gas_limit,
            kind: *to,
            value: *value,
            data: input.clone(),
            ..Default::default()
        }
    }
}

impl FromTxWithEncoded<OpTxEnvelope> for TxEnv {
    fn from_encoded_tx(tx: &OpTxEnvelope, caller: Address, _encoded: Bytes) -> Self {
        Self::from_recovered_tx(tx, caller)
    }
}

impl FromRecoveredTx<OpTxEnvelope> for OpTransaction<TxEnv> {
    fn from_recovered_tx(tx: &OpTxEnvelope, sender: Address) -> Self {
        let encoded = tx.encoded_2718();
        Self::from_encoded_tx(tx, sender, encoded.into())
    }
}

impl FromTxWithEncoded<OpTxEnvelope> for OpTransaction<TxEnv> {
    fn from_encoded_tx(tx: &OpTxEnvelope, caller: Address, encoded: Bytes) -> Self {
        match tx {
            OpTxEnvelope::Legacy(tx) => Self::from_encoded_tx(tx, caller, encoded),
            OpTxEnvelope::Eip1559(tx) => Self::from_encoded_tx(tx, caller, encoded),
            OpTxEnvelope::Eip2930(tx) => Self::from_encoded_tx(tx, caller, encoded),
            OpTxEnvelope::Eip7702(tx) => Self::from_encoded_tx(tx, caller, encoded),
            OpTxEnvelope::Deposit(tx) => Self::from_encoded_tx(tx.inner(), caller, encoded),
        }
    }
}

impl FromRecoveredTx<Signed<TxLegacy>> for OpTransaction<TxEnv> {
    fn from_recovered_tx(tx: &Signed<TxLegacy>, sender: Address) -> Self {
        let encoded = tx.encoded_2718();
        Self::from_encoded_tx(tx, sender, encoded.into())
    }
}

impl FromTxWithEncoded<Signed<TxLegacy>> for OpTransaction<TxEnv> {
    fn from_encoded_tx(tx: &Signed<TxLegacy>, caller: Address, encoded: Bytes) -> Self {
        Self::from_encoded_tx(tx.tx(), caller, encoded)
    }
}

impl FromTxWithEncoded<TxLegacy> for OpTransaction<TxEnv> {
    fn from_encoded_tx(tx: &TxLegacy, caller: Address, encoded: Bytes) -> Self {
        let base = TxEnv::from_recovered_tx(tx, caller);
        Self { base, enveloped_tx: Some(encoded), deposit: Default::default() }
    }
}

impl FromRecoveredTx<Signed<TxEip2930>> for OpTransaction<TxEnv> {
    fn from_recovered_tx(tx: &Signed<TxEip2930>, sender: Address) -> Self {
        let encoded = tx.encoded_2718();
        Self::from_encoded_tx(tx, sender, encoded.into())
    }
}

impl FromTxWithEncoded<Signed<TxEip2930>> for OpTransaction<TxEnv> {
    fn from_encoded_tx(tx: &Signed<TxEip2930>, caller: Address, encoded: Bytes) -> Self {
        Self::from_encoded_tx(tx.tx(), caller, encoded)
    }
}

impl FromTxWithEncoded<TxEip2930> for OpTransaction<TxEnv> {
    fn from_encoded_tx(tx: &TxEip2930, caller: Address, encoded: Bytes) -> Self {
        let base = TxEnv::from_recovered_tx(tx, caller);
        Self { base, enveloped_tx: Some(encoded), deposit: Default::default() }
    }
}

impl FromRecoveredTx<Signed<TxEip1559>> for OpTransaction<TxEnv> {
    fn from_recovered_tx(tx: &Signed<TxEip1559>, sender: Address) -> Self {
        let encoded = tx.encoded_2718();
        Self::from_encoded_tx(tx, sender, encoded.into())
    }
}

impl FromTxWithEncoded<Signed<TxEip1559>> for OpTransaction<TxEnv> {
    fn from_encoded_tx(tx: &Signed<TxEip1559>, caller: Address, encoded: Bytes) -> Self {
        Self::from_encoded_tx(tx.tx(), caller, encoded)
    }
}

impl FromTxWithEncoded<TxEip1559> for OpTransaction<TxEnv> {
    fn from_encoded_tx(tx: &TxEip1559, caller: Address, encoded: Bytes) -> Self {
        let base = TxEnv::from_recovered_tx(tx, caller);
        Self { base, enveloped_tx: Some(encoded), deposit: Default::default() }
    }
}

impl FromRecoveredTx<Signed<TxEip4844>> for OpTransaction<TxEnv> {
    fn from_recovered_tx(tx: &Signed<TxEip4844>, sender: Address) -> Self {
        let encoded = tx.encoded_2718();
        Self::from_encoded_tx(tx, sender, encoded.into())
    }
}

impl FromTxWithEncoded<Signed<TxEip4844>> for OpTransaction<TxEnv> {
    fn from_encoded_tx(tx: &Signed<TxEip4844>, caller: Address, encoded: Bytes) -> Self {
        Self::from_encoded_tx(tx.tx(), caller, encoded)
    }
}

impl FromTxWithEncoded<TxEip4844> for OpTransaction<TxEnv> {
    fn from_encoded_tx(tx: &TxEip4844, caller: Address, encoded: Bytes) -> Self {
        let base = TxEnv::from_recovered_tx(tx, caller);
        Self { base, enveloped_tx: Some(encoded), deposit: Default::default() }
    }
}

impl FromRecoveredTx<Signed<TxEip7702>> for OpTransaction<TxEnv> {
    fn from_recovered_tx(tx: &Signed<TxEip7702>, sender: Address) -> Self {
        let encoded = tx.encoded_2718();
        Self::from_encoded_tx(tx, sender, encoded.into())
    }
}

impl FromTxWithEncoded<Signed<TxEip7702>> for OpTransaction<TxEnv> {
    fn from_encoded_tx(tx: &Signed<TxEip7702>, caller: Address, encoded: Bytes) -> Self {
        Self::from_encoded_tx(tx.tx(), caller, encoded)
    }
}

impl FromTxWithEncoded<TxEip7702> for OpTransaction<TxEnv> {
    fn from_encoded_tx(tx: &TxEip7702, caller: Address, encoded: Bytes) -> Self {
        let base = TxEnv::from_recovered_tx(tx, caller);
        Self { base, enveloped_tx: Some(encoded), deposit: Default::default() }
    }
}

impl FromRecoveredTx<TxDeposit> for OpTransaction<TxEnv> {
    fn from_recovered_tx(tx: &TxDeposit, sender: Address) -> Self {
        let encoded = tx.encoded_2718();
        Self::from_encoded_tx(tx, sender, encoded.into())
    }
}

impl FromTxWithEncoded<TxDeposit> for OpTransaction<TxEnv> {
    fn from_encoded_tx(tx: &TxDeposit, caller: Address, encoded: Bytes) -> Self {
        let base = TxEnv::from_recovered_tx(tx, caller);
        let deposit = DepositTransactionParts {
            source_hash: tx.source_hash,
            mint: Some(tx.mint),
            is_system_transaction: tx.is_system_transaction,
        };
        Self { base, enveloped_tx: Some(encoded), deposit }
    }
}
