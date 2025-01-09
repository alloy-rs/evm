//! Block execution and state transition logic.
//!
//! Block execution is generally composed of the following steps:
//!
//! - System operations: Apply system transactions.
//! - Transaction execution: Execute transactions in the block.
//! - Post transaction execution: Apply post-transaction operations, (e.g. rewards, withdrawals).

// TODO: we need something similar to reth's executor trait that operates on a block, this crate
//  should provide the abstraction and reference implementation for optimism and ethereum, now that
//  we reuse all the alloy types in reth, we should be able to make use of all the alloy-consensus
//  types.

// TODO: ideally of this can also be reused for block building, although this could quickly become
//  complex unless we operate on something that yields `Tx`, and not blocks, this iterator style
//  type  must be aware of things like `gas_used`, invalid tx, etc... so that we can properly build
// a  block.

use crate::evm::Evm;

/// Abstraction over block execution input.
///
/// This type must contain all of the context required to configure EVM and a way to obtain
/// transactions for the block.
pub trait BlockExecutionInput {
    /// Transaction type.
    type Transaction;
}

/// Abstraction over block execution outcome.
pub trait BlockExecutionOutcome {
    /// Receipt type.
    type Receipt;

    fn receipts(&self) -> &[Self::Receipt];
}

/// Abstraction over type that is capable of executing a block.
///
/// This type knows how to configure an underlying EVM and execute transactions on top of it along
/// with any additional pre/post execution actions.
pub trait BlockExecutor {
    /// Input for the block execution.
    type Input: BlockExecutionInput;

    /// Outcome of the block execution.
    type Output: BlockExecutionOutcome;

    /// Errors that can occur during block execution.
    type Error;

    fn execute(&mut self, input: Self::Input) -> Result<Self::Output, Self::Error>;
}

#[cfg(test)]
mod tests {
    use crate::evm::{self, EvmFactory};

    use super::*;

    trait ReceiptBuilder<Tx, EvmRes> {
        type Receipt: Default;

        fn build_receipt<'a>(
            &self,
            ctx: &'a BlockExecutorContext,
            tx: &'a Tx,
            evm_res: &'a EvmRes,
        ) -> Self::Receipt;
    }

    #[derive(Default)]
    struct BlockExecutorContext {
        gas_used: u64,
        blob_gas_used: u64,
    }

    struct BlockExecutorOutput<T> {
        receipts: Vec<T>,
    }

    struct EthBlockExecutor<EvmF, ReceiptB, T> {
        evm_factory: EvmF,
        receipt_builder: ReceiptB,
        _pd: core::marker::PhantomData<T>,
    }

    impl<EvmF, ReceiptB, T> BlockExecutor for EthBlockExecutor<EvmF, ReceiptB, T>
    where
        EvmF: EvmFactory<Evm: Evm<Tx: From<T>>>,
        ReceiptB: ReceiptBuilder<T, <EvmF::Evm as Evm>::Outcome>,
    {
        type Input = alloy_consensus::Block<T>;
        type Output = BlockExecutorOutput<ReceiptB::Receipt>;
        type Error = <EvmF::Evm as Evm>::Error;

        fn execute(&mut self, input: Self::Input) -> Result<Self::Output, Self::Error> {
            // This should use block header and additional context (chainspec?) to create the evm
            // config.
            let evm_input: EvmF::Input = todo!();
            let mut evm = self.evm_factory.create_evm(evm_input);

            let mut ctx = BlockExecutorContext::default();
            let mut receipts = Vec::new();

            for tx in input.body.transactions {
                let result = evm.transact(tx.into())?;
                // ctx.gas_used += result.gas_used();
                // ctx.blob_gas_used += result.blob_gas_used();

                let receipt = self.receipt_builder.build_receipt(&ctx, &tx, &result);
                receipts.push(receipt);
            }

            Ok(BlockExecutorOutput { receipts })
        }
    }

    impl<T> BlockExecutionInput for alloy_consensus::Block<T> {
        type Transaction = T;
    }

    impl<R> BlockExecutionOutcome for BlockExecutorOutput<R> {
        type Receipt = R;

        fn receipts(&self) -> &[Self::Receipt] {
            &self.receipts
        }
    }
}
