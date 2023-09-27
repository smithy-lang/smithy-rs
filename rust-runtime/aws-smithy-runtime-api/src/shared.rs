/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Conversion traits for converting an unshared type into a shared type.
//!
//! The standard [`From`](std::convert::From)/[`Into`](std::convert::Into) traits can't be
//! used for this purpose due to the blanket implementation of `Into`.
//!
//! This implementation also adds a [`maybe_shared`] method and [`impl_shared_conversions`]
//! macro to trivially avoid nesting shared types with other shared types.

use std::any::{Any, TypeId};

/// Like the `From` trait, but for converting to a shared type.
pub trait FromUnshared<Unshared> {
    /// Creates a shared type from an unshared type.
    fn from_unshared(value: Unshared) -> Self;
}

/// Like the `Into` trait, but for (efficiently) converting into a shared type.
///
/// If the type is already a shared type, it won't be nested in another shared type.
pub trait IntoShared<Shared> {
    /// Creates a shared type from an unshared type.
    fn into_shared(self) -> Shared;
}

impl<Unshared, Shared> IntoShared<Shared> for Unshared
where
    Shared: FromUnshared<Unshared>,
{
    fn into_shared(self) -> Shared {
        FromUnshared::from_unshared(self)
    }
}

/// Given a `value`, determine if that value is already shared. If it is, return it. Otherwise, wrap it in a shared type.
pub fn maybe_shared<Shared, MaybeShared, F>(value: MaybeShared, ctor: F) -> Shared
where
    Shared: 'static,
    MaybeShared: IntoShared<Shared> + 'static,
    F: FnOnce(MaybeShared) -> Shared,
{
    // Check if the type is already a shared type
    if TypeId::of::<MaybeShared>() == TypeId::of::<Shared>() {
        // Convince the compiler it is already a shared type and return it
        let mut placeholder = Some(value);
        let value: Shared = (&mut placeholder as &mut dyn Any)
            .downcast_mut::<Option<Shared>>()
            .expect("type checked above")
            .take()
            .expect("set to Some above");
        value
    } else {
        (ctor)(value)
    }
}

/// Implements `FromUnshared` for a shared type.
///
/// # Example
/// ```rust,no_run
/// use aws_smithy_runtime_api::impl_shared_conversions;
/// use std::sync::Arc;
///
/// trait Thing {}
///
/// struct Thingamajig;
/// impl Thing for Thingamajig {}
///
/// struct SharedThing(Arc<dyn Thing>);
/// impl Thing for SharedThing {}
/// impl SharedThing {
///     fn new(thing: impl Thing + 'static) -> Self {
///         Self(Arc::new(thing))
///     }
/// }
/// impl_shared_conversions!(convert SharedThing from Thing using SharedThing::new);
/// ```
#[macro_export]
macro_rules! impl_shared_conversions {
    (convert $shared_type:ident from $unshared_trait:ident using $ctor:expr) => {
        impl<T> $crate::shared::FromUnshared<T> for $shared_type
        where
            T: $unshared_trait + 'static,
        {
            fn from_unshared(value: T) -> Self {
                $crate::shared::maybe_shared(value, $ctor)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt;
    use std::sync::Arc;

    trait Thing: fmt::Debug {}

    #[derive(Debug)]
    struct Thingamajig;
    impl Thing for Thingamajig {}

    #[derive(Debug)]
    struct SharedThing(Arc<dyn Thing>);
    impl Thing for SharedThing {}
    impl SharedThing {
        fn new(thing: impl Thing + 'static) -> Self {
            Self(Arc::new(thing))
        }
    }
    impl_shared_conversions!(convert SharedThing from Thing using SharedThing::new);

    #[test]
    fn test() {
        let thing = Thingamajig;
        assert_eq!("Thingamajig", format!("{thing:?}"), "precondition");

        let shared_thing: SharedThing = thing.into_shared();
        assert_eq!(
            "SharedThing(Thingamajig)",
            format!("{shared_thing:?}"),
            "precondition"
        );

        let very_shared_thing: SharedThing = shared_thing.into_shared();
        assert_eq!(
            "SharedThing(Thingamajig)",
            format!("{very_shared_thing:?}"),
            "it should not nest the shared thing in another shared thing"
        );
    }
}
