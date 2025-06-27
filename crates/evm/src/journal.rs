//! EVM journal wrapper.

use crate::traits::EvmInternals;

/// A wrapper around a mutable reference to an `EvmInternals` trait object.
///
/// This struct provides a convenient way to work with journal operations
/// through dynamic dispatch.
#[derive(Debug)]
pub struct EvmJournal<'a> {
    /// The underlying EVM internals implementation.
    inner: &'a mut dyn EvmInternals,
}

impl<'a> EvmJournal<'a> {
    /// Creates a new `EvmJournal` from a mutable reference to an `EvmInternals` implementation.
    pub fn new(inner: &'a mut dyn EvmInternals) -> Self {
        Self { inner }
    }
}

impl<'a> core::ops::Deref for EvmJournal<'a> {
    type Target = dyn EvmInternals + 'a;

    fn deref(&self) -> &Self::Target {
        self.inner
    }
}

impl<'a> core::ops::DerefMut for EvmJournal<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner
    }
}