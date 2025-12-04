//! State database abstraction.

use revm::database::{states::bundle_state::BundleRetention, BundleState, State};

/// A type which has the state of the blockchain.
///
/// This trait encapsulates some of the functionality found in [`State`]
pub trait StateDB: revm::Database {
    /// State clear EIP-161 is enabled in Spurious Dragon hardfork.
    fn set_state_clear_flag(&mut self, has_state_clear: bool);

    /// Gets a reference to the internal [`BundleState`]
    fn bundle_state(&self) -> &BundleState;

    /// Gets a mutable reference to the internal [`BundleState`]
    fn bundle_state_mut(&mut self) -> &mut BundleState;

    /// This will not apply any pending [`TransitionState`].
    ///
    /// It is recommended to call [`StateDB::merge_transitions`] before taking the bundle.
    ///
    /// If the `State` has been built with the
    /// [`StateBuilder::with_bundle_prestate`] option, the pre-state will be
    /// taken along with any changes made by [`StateDB::merge_transitions`].
    fn take_bundle(&mut self) -> BundleState {
        core::mem::take(self.bundle_state_mut())
    }

    /// Take all transitions and merge them inside [`BundleState`].
    ///
    /// This action will create final post state and all reverts so that
    /// we at any time revert state of bundle to the state before transition
    /// is applied.
    fn merge_transitions(&mut self, retention: BundleRetention);
}

/// auto_impl unable to reconcile return associated type from supertrait
impl<T: StateDB> StateDB for &mut T {
    fn set_state_clear_flag(&mut self, has_state_clear: bool) {
        StateDB::set_state_clear_flag(*self, has_state_clear);
    }

    fn bundle_state(&self) -> &BundleState {
        StateDB::bundle_state(*self)
    }

    fn bundle_state_mut(&mut self) -> &mut BundleState {
        StateDB::bundle_state_mut(*self)
    }

    fn merge_transitions(&mut self, retention: BundleRetention) {
        StateDB::merge_transitions(*self, retention);
    }
}

impl<DB: revm::Database> StateDB for State<DB> {
    fn set_state_clear_flag(&mut self, has_state_clear: bool) {
        Self::set_state_clear_flag(self, has_state_clear);
    }

    fn bundle_state(&self) -> &BundleState {
        &self.bundle_state
    }

    fn bundle_state_mut(&mut self) -> &mut BundleState {
        &mut self.bundle_state
    }

    fn merge_transitions(&mut self, retention: BundleRetention) {
        Self::merge_transitions(self, retention);
    }
}
