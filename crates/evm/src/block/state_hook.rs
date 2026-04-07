use revm::state::EvmState;

/// A hook that is called after each state change.
pub trait OnStateHook: Send + 'static {
    /// Invoked with the source of the change and the state after each system call.
    fn on_state(&mut self, source: StateChangeSource, state: &EvmState);
}

/// Source of the state change
#[derive(Debug, Clone, Copy)]
pub enum StateChangeSource {
    /// Transaction with its index
    Transaction(usize),
    /// Pre-block state transition
    PreBlock(StateChangePreBlockSource),
    /// Post-block state transition
    PostBlock(StateChangePostBlockSource),
}

/// Source of a pre-block state change caused by an EVM system call that executes before
/// transaction processing begins.
///
/// These system calls are protocol-level EVM invocations that update specific system contracts
/// at the start of each block, prior to any user transactions.
#[derive(Debug, Clone, Copy)]
pub enum StateChangePreBlockSource {
    /// EIP-2935: stores parent block hashes in a system contract for in-EVM access.
    BlockHashesContract,
    /// EIP-4788: stores the parent beacon block root in a system contract, making consensus layer
    /// data available to smart contracts.
    BeaconRootContract,
    /// EIP-7002: triggers the withdrawal requests contract to process any queued validator
    /// withdrawal requests.
    WithdrawalRequestsContract,
    /// A custom pre-block state change not covered by the standard variants.
    Other(&'static str),
}

/// Source of a post-block state change caused by a system call or balance modification that
/// executes after all transactions in the block have been processed.
#[derive(Debug, Clone, Copy)]
pub enum StateChangePostBlockSource {
    /// Balance increments applied for block rewards (e.g. beacon withdrawals, ommer rewards).
    BalanceIncrements,
    /// EIP-7002: processes withdrawal requests contract after block execution.
    WithdrawalRequestsContract,
    /// EIP-7251: processes the consolidation requests contract to handle queued validator
    /// consolidations.
    ConsolidationRequestsContract,
    /// A custom post-block state change not covered by the standard variants.
    Other(&'static str),
}

impl<F> OnStateHook for F
where
    F: FnMut(StateChangeSource, &EvmState) + Send + 'static,
{
    fn on_state(&mut self, source: StateChangeSource, state: &EvmState) {
        self(source, state)
    }
}

/// An [`OnStateHook`] that does nothing.
#[derive(Default, Debug, Clone)]
#[non_exhaustive]
pub struct NoopHook;

impl OnStateHook for NoopHook {
    fn on_state(&mut self, _source: StateChangeSource, _state: &EvmState) {}
}
