pub(crate) trait Validate {
    type Unvalidated;
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum Validated<T: Validate> {
    Validated(T),
    Unvalidated(T::Unvalidated),
}
