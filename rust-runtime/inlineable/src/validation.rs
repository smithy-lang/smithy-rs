pub trait Validate {
    type Unvalidated;
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Validated<T: Validate> {
    Validated(T),
    Unvalidated(T::Unvalidated),
}
