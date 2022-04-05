pub trait Validate {
    type Unvalidated;
}
pub enum Validated<T: Validate> {
    Validated(T),
    Unvalidated(T::Unvalidated),
}
