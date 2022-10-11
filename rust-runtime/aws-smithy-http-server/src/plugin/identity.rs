use crate::operation::Operation;

use super::Plugin;

/// An [`Plugin`] that maps an `input` [`Operation`] to itself.
pub struct IdentityPlugin;

impl<P, Op, S, L> Plugin<P, Op, S, L> for IdentityPlugin {
    type Service = S;
    type Layer = L;

    fn map(&self, input: Operation<S, L>) -> Operation<S, L> {
        input
    }
}
