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
/// `allowance = (account.balance - tx.value - max_blob_fee) / tx.gas_price`, where `max_blob_fee`
/// is the blob gas times the blob fee cap that a blob transaction must also fund.
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
    // Subtract the transferred value and the maximum blob fee from the caller balance. Return
    // error if the caller has insufficient funds.
    let blob_fee = if env.tx_type() == TransactionType::Eip4844 {
        env.calc_max_data_fee()
    } else {
        U256::ZERO
    };
    let cost =
        value.checked_add(blob_fee).ok_or(InsufficientFundsError { cost: U256::MAX, balance })?;
    let balance = balance.checked_sub(cost).ok_or(InsufficientFundsError { cost, balance })?;

    Ok(balance
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
        context::TxEnv,
        context_interface::Transaction,
        database::{CacheDB, EmptyDB},
        state::AccountInfo,
    };

    fn allowance(balance: U256, tx: TxEnv) -> Result<u64, CallError<core::convert::Infallible>> {
        let mut db = CacheDB::new(EmptyDB::new());
        db.insert_account_info(tx.caller, AccountInfo { balance, ..Default::default() });
        caller_gas_allowance(&mut db, &tx)
    }

    fn blob_tx(tx_type: TransactionType) -> TxEnv {
        TxEnv {
            tx_type: tx_type as u8,
            caller: Address::repeat_byte(0x11),
            gas_price: 1,
            blob_hashes: alloc::vec![B256::repeat_byte(0x01)],
            max_fee_per_blob_gas: 1,
            ..Default::default()
        }
    }

    #[test]
    fn blob_fee_is_reserved_before_dividing_by_gas_price() {
        let tx = blob_tx(TransactionType::Eip4844);
        let blob_fee = tx.calc_max_data_fee().to::<u64>();

        assert_eq!(allowance(U256::from(1_000_000), tx.clone()).unwrap(), 1_000_000 - blob_fee);
        assert!(matches!(
            allowance(U256::from(blob_fee - 1), tx),
            Err(CallError::InsufficientFunds(_))
        ));
    }

    #[test]
    fn blob_fields_of_other_types_are_not_reserved() {
        assert_eq!(
            allowance(U256::from(1_000_000), blob_tx(TransactionType::Eip7702)).unwrap(),
            1_000_000
        );
    }

    #[test]
    fn value_and_blob_fee_overflow_is_insufficient_funds() {
        let tx = blob_tx(TransactionType::Eip4844);
        let tx = TxEnv { value: U256::MAX - tx.calc_max_data_fee() + U256::from(1), ..tx };
        assert!(matches!(allowance(U256::MAX, tx), Err(CallError::InsufficientFunds(_))));
    }
}
