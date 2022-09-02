use crate::operation::Operation;

pub trait BuildModifier<Protocol, Op, S, L> {
    type Service;
    type Layer;

    fn modify(&self, operation: Operation<S, L>) -> Operation<Self::Service, Self::Layer>;
}

pub struct Identity;

impl<P, Op, S, L> BuildModifier<P, Op, S, L> for Identity {
    type Service = S;
    type Layer = L;

    fn modify(&self, operation: Operation<S, L>) -> Operation<Self::Service, Self::Layer> {
        operation
    }
}
