/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::marker::PhantomData;

use super::Plugin;

pub struct Boolean<const VALUE: bool>;

/// Conditionally applies a [`Plugin`] `Pl` to some service `S`.
///
/// See [`Boolean<true>`] and [`Boolean<false>`].
pub trait ConditionalApply<Ser, Op, Pl, T> {
    type Output;

    fn apply(plugin: &Pl, input: T) -> Self::Output;
}

impl<Ser, Op, Pl, T> ConditionalApply<Ser, Op, Pl, T> for Boolean<true>
where
    Pl: Plugin<Ser, Op, T>,
{
    type Output = Pl::Output;

    fn apply(plugin: &Pl, input: T) -> Self::Output {
        plugin.apply(input)
    }
}

impl<P, Op, Pl, T> ConditionalApply<P, Op, Pl, T> for Boolean<false> {
    type Output = T;

    fn apply(_plugin: &Pl, input: T) -> Self::Output {
        input
    }
}

/// Implements a NAND gate.
const fn nand(a: bool, b: bool) -> bool {
    !(a && b)
}

/// Holds a constant boolean value.
/// It is used to resolve an expression after applying [`nand`].
trait BooleanExpr {
    const RESOLVED: bool;
}

impl<const VALUE: bool> BooleanExpr for Boolean<VALUE> {
    const RESOLVED: bool = VALUE;
}

impl<B1, B2> BooleanExpr for NAnd<B1, B2>
where
    B1: BooleanExpr,
    B2: BooleanExpr,
{
    const RESOLVED: bool = nand(B1::RESOLVED, B2::RESOLVED);
}

/// The NAND gate of two [`BooleanExpr`] boolean expressions.
pub struct NAnd<Boolean1, Boolean2>(pub Boolean1, pub Boolean2);

impl<Ser, Op, Pl, T, B1, B2> ConditionalApply<Ser, Op, Pl, T> for NAnd<B1, B2>
where
    B1: BooleanExpr,
    B2: BooleanExpr,
    Boolean<{ NAnd::<B1, B2>::RESOLVED }>: ConditionalApply<Ser, Op, Pl, T>,
{
    type Output = <Boolean<{ NAnd::<B1, B2>::RESOLVED }> as ConditionalApply<Ser, Op, Pl, T>>::Output;

    fn apply(plugin: &Pl, input: T) -> Self::Output {
        <Boolean<{ NAnd::<B1, B2>::RESOLVED }> as ConditionalApply<Ser, Op, Pl, T>>::apply(plugin, input)
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
    type Output = <Scope::Contains as ConditionalApply<Ser, Op, Pl, T>>::Output;

    fn apply(&self, input: T) -> Self::Output {
        <Scope::Contains as ConditionalApply<Ser, Op, Pl, T>>::apply(&self.plugin, input)
    }
}

/// The complement of a [`Scope`].
/// The operations included in a scope become exludes and vice versa.
struct Complement<Scope>(Scope);

impl<Op, Scope> Membership<Op> for Complement<Scope>
where
    Scope: Membership<Op>,
{
    // Not Scope
    type Contains = NAnd<Scope::Contains, Scope::Contains>;
}

/// The intersection of two [`Scope`]s.
/// All and only the operations included in both scopes are included in this new scopes.
pub struct Intersection<ScopeA, ScopeB>(ScopeA, ScopeB);

impl<Op, ScopeA, ScopeB> Membership<Op> for Intersection<ScopeA, ScopeB>
where
    ScopeA: Membership<Op>,
    ScopeB: Membership<Op>,
{
    // ScopeA and ScopeB
    type Contains = NAnd<NAnd<ScopeA::Contains, ScopeB::Contains>, NAnd<ScopeA::Contains, ScopeB::Contains>>;
}

/// The union of two [`Scope`]s.
/// All and only the operations included in either scopes are included in this new scopes.
pub struct Union<ScopeA, ScopeB>(ScopeA, ScopeB);

impl<Op, ScopeA, ScopeB> Membership<Op> for Union<ScopeA, ScopeB>
where
    ScopeA: Membership<Op>,
    ScopeB: Membership<Op>,
{
    // ScopeA or ScopeB
    type Contains = NAnd<NAnd<ScopeA::Contains, ScopeA::Contains>, NAnd<ScopeB::Contains, ScopeB::Contains>>;
}

/// A macro to help with scoping [plugins](crate::plugin) to a subset of all operations.
///
/// The scope must partition _all_ operations, that is, each and every operation must be included or excluded, but not
/// both.
///
/// The generated server SDK exports a similar `scope` macro which is aware of a services operations and can complete
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
                type Contains = $crate::plugin::scoped::Boolean<true>;
            }
        )*
        $(
            impl $crate::plugin::scoped::Membership<$exclude> for $name {
                type Contains = $crate::plugin::scoped::Boolean<false>;
            }
        )*
    };
}

#[cfg(test)]
mod tests {
    use crate::plugin::Plugin;

    use super::*;

    struct OperationA;
    struct OperationB;
    struct OperationC;

    scope! {
        /// Includes A and B, not C.
        pub struct OnlyAB {
            includes: [OperationA, OperationB],
            excludes: [OperationC]
        }
    }

    scope! {
        // Includes only A.
        pub struct OnlyA {
            includes: [OperationA],
            excludes: [OperationB, OperationC]
        }
    }

    scope! {
        // Includes only A.
        pub struct OnlyC {
            includes: [OperationC],
            excludes: [OperationA, OperationB]
        }
    }

    struct MockPlugin;

    impl<P, Op> Plugin<P, Op, u32> for MockPlugin {
        type Output = String;

        fn apply(&self, input: u32) -> Self::Output {
            input.to_string()
        }
    }

    #[test]
    fn scope() {
        let plugin = MockPlugin;
        let scoped_plugin = Scoped::new::<OnlyAB>(plugin);

        let out: String = Plugin::<(), OperationA, _>::apply(&scoped_plugin, 3_u32);
        assert_eq!(out, "3".to_string());
        let out: String = Plugin::<(), OperationB, _>::apply(&scoped_plugin, 3_u32);
        assert_eq!(out, "3".to_string());
        let out: u32 = Plugin::<(), OperationC, _>::apply(&scoped_plugin, 3_u32);
        assert_eq!(out, 3);
    }

    #[test]
    fn combinator() {
        let plugin = MockPlugin;

        type NewScope = Intersection<OnlyAB, OnlyA>;
        type NewScope2 = Union<NewScope, OnlyC>;
        type NewScope3 = Complement<NewScope2>;
        type NewScope4 = Complement<NewScope3>;
        let scoped_plugin = Scoped::new::<NewScope4>(plugin);

        let out: String = Plugin::<(), OperationA, _>::apply(&scoped_plugin, 3_u32);
        assert_eq!(out, "3".to_string());
        let out: u32 = Plugin::<(), OperationB, _>::apply(&scoped_plugin, 3_u32);
        assert_eq!(out, 3);
        let out: String = Plugin::<(), OperationC, _>::apply(&scoped_plugin, 3_u32);
        assert_eq!(out, "3".to_string());
    }

    #[test]
    fn intersection() {
        let plugin = MockPlugin;

        let scoped_plugin = Scoped::new::<Intersection<OnlyAB, OnlyA>>(plugin);

        let out: String = Plugin::<(), OperationA, _>::apply(&scoped_plugin, 3_u32);
        assert_eq!(out, "3".to_string());
        let out: u32 = Plugin::<(), OperationB, _>::apply(&scoped_plugin, 3_u32);
        assert_eq!(out, 3);
        let out: u32 = Plugin::<(), OperationC, _>::apply(&scoped_plugin, 3_u32);
        assert_eq!(out, 3);
    }

    #[test]
    fn union() {
        let plugin = MockPlugin;

        let scoped_plugin = Scoped::new::<Union<OnlyAB, OnlyC>>(plugin);

        let out: String = Plugin::<(), OperationA, _>::apply(&scoped_plugin, 3_u32);
        assert_eq!(out, "3".to_string());
        let out: String = Plugin::<(), OperationB, _>::apply(&scoped_plugin, 3_u32);
        assert_eq!(out, "3".to_string());
        let out: String = Plugin::<(), OperationC, _>::apply(&scoped_plugin, 3_u32);
        assert_eq!(out, "3".to_string());
    }

    #[test]
    fn complement() {
        let plugin = MockPlugin;

        let scoped_plugin = Scoped::new::<Complement<OnlyAB>>(plugin);

        let out: u32 = Plugin::<(), OperationA, _>::apply(&scoped_plugin, 3_u32);
        assert_eq!(out, 3);
        let out: u32 = Plugin::<(), OperationB, _>::apply(&scoped_plugin, 3_u32);
        assert_eq!(out, 3);
        let out: String = Plugin::<(), OperationC, _>::apply(&scoped_plugin, 3_u32);
        assert_eq!(out, "3".to_string());
    }
}
