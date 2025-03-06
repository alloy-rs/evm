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

/// Source of the pre-block state change
#[derive(Debug, Clone, Copy)]
pub enum StateChangePreBlockSource {
    /// EIP-2935 blockhashes contract
    BlockHashesContract,
    /// EIP-4788 beacon root contract
    BeaconRootContract,
    /// EIP-7002 withdrawal requests contract
    WithdrawalRequestsContract,
}

/// Source of the post-block state change
#[derive(Debug, Clone, Copy)]
pub enum StateChangePostBlockSource {
    /// Balance increments from block rewards and withdrawals
    BalanceIncrements,
    /// EIP-7002 withdrawal requests contract
    WithdrawalRequestsContract,
    /// EIP-7251 consolidation requests contract
    ConsolidationRequestsContract,
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
