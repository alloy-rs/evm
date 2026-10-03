//! Utilities for dealing with eth_call and adjacent RPC endpoints.

use alloy_primitives::U256;
use revm::{context_interface::transaction::TransactionType, Database};

/// Insufficient funds error
#[derive(Debug, thiserror::Error)]
#[error("insufficient funds: cost {cost} > balance {balance}")]
pub struct InsufficientFundsError {
    /// Transaction cost
    pub cost: U256,
    /// Account balance
    pub balance: U256,
}

/// Error type for call utilities
#[derive(Debug, thiserror::Error)]
pub enum CallError<E> {
    /// Database error
    #[error(transparent)]
    Database(E),
    /// Insufficient funds error
    #[error(transparent)]
    InsufficientFunds(#[from] InsufficientFundsError),
}

/// Calculates the caller gas allowance.
///
/// `allowance = (account.balance - tx.value - max_blob_fee) / tx.gas_price`
///
/// For EIP-4844 transactions, reserves `max_fee_per_blob_gas * total_blob_gas` before
/// allocating the remaining balance to execution gas.
///
/// Returns an error if the caller has insufficient funds.
/// Caution: This assumes non-zero `env.gas_price`. Otherwise, zero allowance will be returned.
///
/// Note: this takes the mut [Database] trait because the loaded sender can be reused for the
/// following operation like `eth_call`.
pub fn caller_gas_allowance<DB, T>(db: &mut DB, env: &T) -> Result<u64, CallError<DB::Error>>
where
    DB: Database,
    T: revm::context_interface::Transaction,
{
    // Get the caller account.
    let caller = db.basic(env.caller()).map_err(CallError::Database)?;
    // Get the caller balance.
    let balance = caller.map(|acc| acc.balance).unwrap_or_default();
    // Get transaction value.
    let value = env.value();
    let blob_fee = if env.tx_type() == TransactionType::Eip4844 {
        env.calc_max_data_fee()
    } else {
        U256::ZERO
    };
    // Reserve the value and maximum blob fee, matching the EVM's upfront funding check.
    // Subtract separately so an overflowing total cost cannot become an affordable allowance.
    let available = balance
        .checked_sub(value)
        .and_then(|balance| balance.checked_sub(blob_fee))
        .ok_or(InsufficientFundsError { cost: value.saturating_add(blob_fee), balance })?;

    Ok(available
        // Calculate the amount of gas the caller can afford with the specified gas price.
        .checked_div(U256::from(env.gas_price()))
        // This will be 0 if gas price is 0. It is fine, because we check it before.
        .unwrap_or_default()
        .saturating_to())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{Address, B256};
    use revm::{
        context::TxEnv, context_interface::Transaction, database::InMemoryDB, state::AccountInfo,
    };

    #[test]
    fn gas_allowance_reserves_max_blob_fees() {
        let caller = Address::repeat_byte(0x11);
        let balance = U256::from(1_000_000);
        let mut db = InMemoryDB::default();
        db.insert_account_info(caller, AccountInfo { balance, ..Default::default() });

        for blob_count in [1, 2] {
            for blob_fee_cap in [0, 1, 3] {
                let mut tx = TxEnv {
                    tx_type: TransactionType::Eip4844 as u8,
                    caller,
                    gas_price: 10,
                    value: U256::from(1_000),
                    blob_hashes: vec![B256::repeat_byte(1); blob_count],
                    max_fee_per_blob_gas: blob_fee_cap,
                    ..Default::default()
                };

                tx.gas_limit = caller_gas_allowance(&mut db, &tx).unwrap();
                assert_eq!(
                    tx.gas_limit,
                    (999_000 - 131_072 * blob_count as u64 * blob_fee_cap as u64) / 10
                );
                tx.ensure_enough_balance(balance).unwrap();
                tx.gas_limit += 1;
                assert!(tx.ensure_enough_balance(balance).is_err());
            }
        }
    }

    #[test]
    fn gas_allowance_rejects_unaffordable_value_and_blobs() {
        let caller = Address::repeat_byte(0x11);
        for (balance, value) in [
            (U256::from(1_000), U256::from(1_001)),
            (U256::from(131_071), U256::ZERO),
            (U256::from(132_071), U256::from(1_000)),
            (U256::MAX, U256::MAX),
        ] {
            let mut db = InMemoryDB::default();
            db.insert_account_info(caller, AccountInfo { balance, ..Default::default() });
            let tx = TxEnv {
                tx_type: TransactionType::Eip4844 as u8,
                caller,
                gas_price: 10,
                value,
                blob_hashes: vec![B256::repeat_byte(1)],
                max_fee_per_blob_gas: 1,
                ..Default::default()
            };

            let err = caller_gas_allowance(&mut db, &tx).unwrap_err();
            assert!(matches!(err, CallError::InsufficientFunds(err)
                if err.balance == balance && err.cost == value.saturating_add(U256::from(131_072))));
        }
    }

    #[test]
    fn gas_allowance_ignores_blob_fields_on_other_transaction_types() {
        let caller = Address::repeat_byte(0x11);
        let mut db = InMemoryDB::default();
        db.insert_account_info(
            caller,
            AccountInfo { balance: U256::from(1_000_000), ..Default::default() },
        );
        let tx = TxEnv {
            tx_type: TransactionType::Eip1559 as u8,
            caller,
            gas_price: 10,
            value: U256::from(1_000),
            blob_hashes: vec![B256::repeat_byte(1)],
            max_fee_per_blob_gas: 1,
            ..Default::default()
        };

        assert_eq!(caller_gas_allowance(&mut db, &tx).unwrap(), 99_900);
    }
}
