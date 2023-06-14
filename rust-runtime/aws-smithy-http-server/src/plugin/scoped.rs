/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::marker::PhantomData;

use super::Plugin;

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
pub trait ConditionalApply<P, Op, S, Pl> {
    type Service;

    fn apply(plugin: &Pl, svc: S) -> Self::Service;
}

impl<P, Op, S, Pl> ConditionalApply<P, Op, S, Pl> for True
where
    Pl: Plugin<P, Op, S>,
{
    type Service = Pl::Service;

    fn apply(plugin: &Pl, svc: S) -> Self::Service {
        plugin.apply(svc)
    }
}

impl<P, Op, S, Pl> ConditionalApply<P, Op, S, Pl> for False {
    type Service = S;

    fn apply(_plugin: &Pl, svc: S) -> Self::Service {
        svc
    }
}

/// A [`Plugin`] which scopes the application of an inner [`Plugin`].
///
/// Operations within the scope will have the inner [`Plugin`] applied.
///
/// # Example
///
/// ```rust
/// # struct OperationA;
/// # struct OperationB;
/// # struct OperationC;
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

impl<P, Op, S, Scope, Pl> Plugin<P, Op, S> for Scoped<Scope, Pl>
where
    Scope: Membership<Op>,
    Scope::Contains: ConditionalApply<P, Op, S, Pl>,
    Pl: Plugin<P, Op, S>,
{
    type Service = <Scope::Contains as ConditionalApply<P, Op, S, Pl>>::Service;

    fn apply(&self, svc: S) -> Self::Service {
        <Scope::Contains as ConditionalApply<P, Op, S, Pl>>::apply(&self.plugin, svc)
    }
}

/// A macro to help with scoping plugins to a subset of all operations.
///
/// The scope must partition _all_ operations, that is, each and every operation must be included or excluded, but not both.
///
/// # Example
///
/// For a service with three operations: `OperationA`, `OperationB`, `OperationC`.
///
/// ```rust
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
        type Service = String;

        fn apply(&self, svc: u32) -> Self::Service {
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
