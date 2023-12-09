/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::marker::PhantomData;

use super::{HttpMarker, ModelMarker, Plugin};

/// Marker struct for `true`.
///
/// Implements [`ConditionalApply`] which applies the [`Plugin`].
pub struct True;

/// Marker struct for `false`.
///
/// Implements [`ConditionalApply`] which does nothing.
pub struct False;

/// Conditionally applies a [`Plugin`] `Pl` to some service `S`.
///
/// See [`True`] and [`False`].
pub trait ConditionalApply<Ser, Op, Pl, T> {
    type Service;

    fn apply(plugin: &Pl, svc: T) -> Self::Service;
}

impl<Ser, Op, Pl, T> ConditionalApply<Ser, Op, Pl, T> for True
where
    Pl: Plugin<Ser, Op, T>,
{
    type Service = Pl::Output;

    fn apply(plugin: &Pl, input: T) -> Self::Service {
        plugin.apply(input)
    }
}

impl<P, Op, Pl, T> ConditionalApply<P, Op, Pl, T> for False {
    type Service = T;

    fn apply(_plugin: &Pl, input: T) -> Self::Service {
        input
    }
}

/// A [`Plugin`] which scopes the application of an inner [`Plugin`].
///
/// In cases where operation selection must be performed at runtime [`filter_by_operation`](crate::plugin::filter_by_operation)
/// can be used.
///
/// Operations within the scope will have the inner [`Plugin`] applied.
///
/// # Example
///
/// ```rust
/// # use aws_smithy_http_server::{scope, plugin::Scoped};
/// # struct OperationA; struct OperationB; struct OperationC;
/// # let plugin = ();
///
/// // Define a scope over a service with 3 operations
/// scope! {
///     struct OnlyAB {
///         includes: [OperationA, OperationB],
///         excludes: [OperationC]
///     }
/// }
///
/// // Create a scoped plugin
/// let scoped_plugin = Scoped::new::<OnlyAB>(plugin);
/// ```
pub struct Scoped<Scope, Pl> {
    scope: PhantomData<Scope>,
    plugin: Pl,
}

impl<Pl> Scoped<(), Pl> {
    /// Creates a new [`Scoped`] from a `Scope` and [`Plugin`].
    pub fn new<Scope>(plugin: Pl) -> Scoped<Scope, Pl> {
        Scoped {
            scope: PhantomData,
            plugin,
        }
    }
}

/// A trait marking which operations are in scope via the associated type [`Membership::Contains`].
pub trait Membership<Op> {
    type Contains;
}

impl<Ser, Op, T, Scope, Pl> Plugin<Ser, Op, T> for Scoped<Scope, Pl>
where
    Scope: Membership<Op>,
    Scope::Contains: ConditionalApply<Ser, Op, Pl, T>,
{
    type Output = <Scope::Contains as ConditionalApply<Ser, Op, Pl, T>>::Service;

    fn apply(&self, input: T) -> Self::Output {
        <Scope::Contains as ConditionalApply<Ser, Op, Pl, T>>::apply(&self.plugin, input)
    }
}

impl<Scope, Pl> HttpMarker for Scoped<Scope, Pl> where Pl: HttpMarker {}
impl<Scope, Pl> ModelMarker for Scoped<Scope, Pl> where Pl: ModelMarker {}

/// A macro to help with scoping [plugins](crate::plugin) to a subset of all operations.
///
/// The scope must partition _all_ operations, that is, each and every operation must be included or excluded, but not
/// both.
///
/// The generated server SDK exports a similar `scope` macro which is aware of a service's operations and can complete
/// underspecified scopes automatically.
///
/// # Example
///
/// For a service with three operations: `OperationA`, `OperationB`, `OperationC`.
///
/// ```rust
/// # use aws_smithy_http_server::scope;
/// # struct OperationA; struct OperationB; struct OperationC;
/// scope! {
///     struct OnlyAB {
///         includes: [OperationA, OperationB],
///         excludes: [OperationC]
///     }
/// }
/// ```
#[macro_export]
macro_rules! scope {
    (
        $(#[$attrs:meta])*
        $vis:vis struct $name:ident {
            includes: [$($include:ty),*],
            excludes: [$($exclude:ty),*]
        }
    ) => {
        $(#[$attrs])*
        $vis struct $name;

        $(
            impl $crate::plugin::scoped::Membership<$include> for $name {
                type Contains = $crate::plugin::scoped::True;
            }
        )*
        $(
            impl $crate::plugin::scoped::Membership<$exclude> for $name {
                type Contains = $crate::plugin::scoped::False;
            }
        )*
    };
}

#[cfg(test)]
mod tests {
    use crate::plugin::Plugin;

    use super::Scoped;

    struct OperationA;
    struct OperationB;

    scope! {
        /// Includes A, not B.
        pub struct AuthScope {
            includes: [OperationA],
            excludes: [OperationB]
        }
    }

    struct MockPlugin;

    impl<P, Op> Plugin<P, Op, u32> for MockPlugin {
        type Output = String;

        fn apply(&self, svc: u32) -> Self::Output {
            svc.to_string()
        }
    }

    #[test]
    fn scope() {
        let plugin = MockPlugin;
        let scoped_plugin = Scoped::new::<AuthScope>(plugin);

        let out: String = Plugin::<(), OperationA, _>::apply(&scoped_plugin, 3_u32);
        assert_eq!(out, "3".to_string());
        let out: u32 = Plugin::<(), OperationB, _>::apply(&scoped_plugin, 3_u32);
        assert_eq!(out, 3);
    }
}
