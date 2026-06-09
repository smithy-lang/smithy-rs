/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::fmt;

/// Creates a [`ShapeId`] from a namespace and shape name at compile time.
///
/// The fully qualified name (`namespace#ShapeName`) is computed via `concat!`,
/// eliminating the risk of the FQN getting out of sync with the parts.
///
/// # Examples
/// ```
/// use aws_smithy_schema::{shape_id, ShapeId};
///
/// const ID: ShapeId<'static> = shape_id!("smithy.api", "String");
/// assert_eq!(ID.as_str(), "smithy.api#String");
/// ```
#[macro_export]
macro_rules! shape_id {
    ($ns:literal, $name:literal) => {
        $crate::ShapeId::from_static(concat!($ns, "#", $name), $ns, $name)
    };
    ($ns:literal, $name:literal, $member:literal) => {
        $crate::ShapeId::from_static_with_member(
            concat!($ns, "#", $name, "$", $member),
            $ns,
            $name,
            $member,
        )
    };
}

/// A Smithy Shape ID, parameterized over the lifetime of its component
/// strings.
///
/// `ShapeId<'static>` is the codegen-emitted form (zero-allocation,
/// const-constructible via the [`shape_id!`] macro). `ShapeId<'a>` for
/// shorter `'a` is constructible from runtime-parsed bytes — for example,
/// from a wire-format `__type` field on a JSON response.
///
/// Use the [`shape_id!`] macro to construct `'static` instances — it
/// computes the fully qualified name at compile time from the namespace
/// and shape name, preventing the parts from getting out of sync.
///
/// # Examples
/// ```
/// use aws_smithy_schema::{shape_id, ShapeId};
///
/// const ID: ShapeId<'static> = shape_id!("smithy.api", "String");
/// assert_eq!(ID.namespace(), "smithy.api");
/// assert_eq!(ID.shape_name(), "String");
/// assert_eq!(ID.as_str(), "smithy.api#String");
/// ```
///
/// # Variance
///
/// `ShapeId<'a>` is **covariant** in `'a` because every field is of the
/// form `&'a str`. This means a `ShapeId<'static>` (codegen-emitted) is
/// usable everywhere a `ShapeId<'a>` is expected, regardless of `'a`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShapeId<'a> {
    fqn: &'a str,
    namespace: &'a str,
    shape_name: &'a str,
    member_name: Option<&'a str>,
}

impl<'a> ShapeId<'a> {
    /// Creates a ShapeId from pre-computed strings.
    ///
    /// The function name is historical: when this method was introduced,
    /// all `ShapeId` references were `'static`. Today `ShapeId<'a>` accepts
    /// any lifetime; the function still compiles in `const` contexts when
    /// the inputs are `'static` (which is the codegen case).
    ///
    /// Prefer the [`shape_id!`] macro which computes `fqn` via `concat!`
    /// to prevent the parts from getting out of sync.
    #[doc(hidden)]
    pub const fn from_static(fqn: &'a str, namespace: &'a str, shape_name: &'a str) -> Self {
        Self {
            fqn,
            namespace,
            shape_name,
            member_name: None,
        }
    }

    /// Creates a ShapeId with a member name from pre-computed strings.
    ///
    /// See [`Self::from_static`] for naming rationale.
    ///
    /// Prefer the [`shape_id!`] macro which computes `fqn` via `concat!`
    /// to prevent the parts from getting out of sync.
    #[doc(hidden)]
    pub const fn from_static_with_member(
        fqn: &'a str,
        namespace: &'a str,
        shape_name: &'a str,
        member_name: &'a str,
    ) -> Self {
        Self {
            fqn,
            namespace,
            shape_name,
            member_name: Some(member_name),
        }
    }

    /// Returns the fully qualified string representation (e.g. `"smithy.api#String"`).
    ///
    /// The return type is `&'a str` (the lifetime of the data, not the
    /// receiver). Calling this on `ShapeId<'static>` yields `&'static str`,
    /// preserving the codegen-side `'static` accessor guarantee.
    pub fn as_str(&self) -> &'a str {
        self.fqn
    }

    /// Returns the namespace portion of the ShapeId.
    ///
    /// Lifetime is `'a` — see [`Self::as_str`] for rationale.
    pub fn namespace(&self) -> &'a str {
        self.namespace
    }

    /// Returns the shape name portion of the ShapeId.
    ///
    /// Lifetime is `'a` — see [`Self::as_str`] for rationale.
    pub fn shape_name(&self) -> &'a str {
        self.shape_name
    }

    /// Returns the member name if this is a member shape ID.
    ///
    /// Lifetime is `'a` — see [`Self::as_str`] for rationale.
    pub fn member_name(&self) -> Option<&'a str> {
        self.member_name
    }
}

impl fmt::Display for ShapeId<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.fqn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shape_id_macro() {
        const ID: ShapeId<'static> = shape_id!("smithy.api", "String");
        assert_eq!(ID.as_str(), "smithy.api#String");
        assert_eq!(ID.namespace(), "smithy.api");
        assert_eq!(ID.shape_name(), "String");
        assert_eq!(ID.member_name(), None);
    }

    #[test]
    fn test_shape_id_macro_with_member() {
        const ID: ShapeId<'static> = shape_id!("com.example", "MyStruct", "field");
        assert_eq!(ID.as_str(), "com.example#MyStruct$field");
        assert_eq!(ID.namespace(), "com.example");
        assert_eq!(ID.shape_name(), "MyStruct");
        assert_eq!(ID.member_name(), Some("field"));
    }

    #[test]
    fn test_display() {
        let id = shape_id!("smithy.api", "String");
        assert_eq!(format!("{id}"), "smithy.api#String");
    }

    #[test]
    fn test_equality() {
        let a = shape_id!("smithy.api", "String");
        let b = shape_id!("smithy.api", "String");
        assert_eq!(a, b);

        let c = shape_id!("smithy.api", "String", "foo");
        let d = shape_id!("smithy.api", "String", "foo");
        assert_eq!(c, d);
    }

    /// Construct a `ShapeId<'a>` from non-`'static` data. Compile-tests
    /// that the lifetime parameter does what we want — without this the
    /// runtime-built ShapeId path would not be exercised by tests.
    #[test]
    fn test_runtime_lifetime() {
        let fqn = String::from("ns#Foo");
        let ns = String::from("ns");
        let name = String::from("Foo");
        let id: ShapeId<'_> = ShapeId::from_static(&fqn, &ns, &name);
        assert_eq!(id.as_str(), "ns#Foo");
        assert_eq!(id.namespace(), "ns");
        assert_eq!(id.shape_name(), "Foo");
    }
}
