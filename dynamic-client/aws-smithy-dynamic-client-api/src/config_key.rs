/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Typed config keys for settings that only some models use.

use aws_smithy_lang::ShapeId;
use aws_smithy_types::config_bag::Storable;
use std::borrow::Cow;
use std::error::Error;
use std::fmt::{self, Debug};

/// A config value that only some models use, such as a value for a Smithy trait.
///
/// The Rust type is the key, so each value is set by its own type through `put(..)` on the
/// dynamic client's config builder. A plugin crate can define its own keys.
///
/// ```
/// use aws_smithy_dynamic_client_api::config_key::{Applies, ConfigKey};
/// use aws_smithy_lang::ShapeId;
/// use aws_smithy_types::config_bag::{Storable, StoreReplace};
///
/// #[derive(Clone, Debug)]
/// struct DisableWidgets(bool);
///
/// impl Storable for DisableWidgets {
///     type Storer = StoreReplace<Self>;
/// }
///
/// impl ConfigKey for DisableWidgets {
///     const APPLIES: &'static [Applies] =
///         &[Applies::Trait(ShapeId::from_static("example.widgets#widget"))];
/// }
/// ```
pub trait ConfigKey: Storable + Clone + Debug {
    /// The conditions that make this key matter for a model.
    ///
    /// The key matters when any condition holds. An empty list means the key always matters.
    const APPLIES: &'static [Applies];
}

/// A condition that makes a [`ConfigKey`] matter for a model.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Applies {
    /// A shape in the model carries this trait.
    Trait(ShapeId),
    /// The endpoint rules declare a parameter with this `builtIn`.
    EndpointBuiltIn(EndpointBuiltIn),
}

/// The name of an endpoint rule-set built-in, such as `AWS::UseFIPS`. A rule-set parameter
/// declares one with its `builtIn` property.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointBuiltIn(Cow<'static, str>);

impl EndpointBuiltIn {
    /// The `AWS::UseFIPS` endpoint rule-set built-in.
    pub const USE_FIPS: Self = Self::from_static("AWS::UseFIPS");
    /// The `AWS::UseDualStack` endpoint rule-set built-in.
    pub const USE_DUAL_STACK: Self = Self::from_static("AWS::UseDualStack");
    /// The `AWS::Auth::AccountIdEndpointMode` endpoint rule-set built-in.
    pub const ACCOUNT_ID_ENDPOINT_MODE: Self =
        Self::from_static("AWS::Auth::AccountIdEndpointMode");

    /// Creates the name of an endpoint rule-set built-in, from a name known at compile time.
    ///
    /// # Panics
    ///
    /// Panics if `name` is not of the form `Prefix::Name`, with no empty part. Used in a
    /// `const`, the panic fails the compile:
    ///
    /// ```compile_fail
    /// use aws_smithy_dynamic_client_api::config_key::EndpointBuiltIn;
    ///
    /// const USE_FIPS: EndpointBuiltIn = EndpointBuiltIn::from_static("UseFIPS");
    /// ```
    pub const fn from_static(name: &'static str) -> Self {
        if !is_built_in_name(name.as_bytes()) {
            panic!("an endpoint built-in name must be of the form `Prefix::Name`");
        }
        Self(Cow::Borrowed(name))
    }

    /// Creates the name of an endpoint rule-set built-in, from a name known only at runtime.
    ///
    /// Use it for endpoint rules built or edited in memory.
    ///
    /// # Errors
    ///
    /// Fails if `name` is not of the form `Prefix::Name`, with no empty part.
    pub fn new(name: impl Into<Cow<'static, str>>) -> Result<Self, InvalidEndpointBuiltIn> {
        let name = name.into();
        if is_built_in_name(name.as_bytes()) {
            Ok(Self(name))
        } else {
            Err(InvalidEndpointBuiltIn { name })
        }
    }

    /// The name of the endpoint rule-set built-in, such as `AWS::UseFIPS`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An error from [`EndpointBuiltIn::new`]: the name is not of the form `Prefix::Name`.
#[derive(Debug)]
pub struct InvalidEndpointBuiltIn {
    name: Cow<'static, str>,
}

impl fmt::Display for InvalidEndpointBuiltIn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is not an endpoint built-in name of the form `Prefix::Name`",
            self.name
        )
    }
}

impl Error for InvalidEndpointBuiltIn {}

// Returns `true` for `A::B` and `A::B::C`: at least two parts, separated by `::`, none empty.
// The rules engine spec requires built-ins to be namespaced with `::`, as in `AWS::UseFIPS`.
const fn is_built_in_name(name: &[u8]) -> bool {
    let mut parts = 1;
    let mut part_len = 0;
    let mut i = 0;
    while i < name.len() {
        if name[i] == b':' {
            if i + 1 >= name.len() || name[i + 1] != b':' || part_len == 0 {
                return false;
            }
            parts += 1;
            part_len = 0;
            i += 2;
        } else {
            part_len += 1;
            i += 1;
        }
    }
    parts >= 2 && part_len > 0
}

#[cfg(test)]
mod tests {
    use super::{is_built_in_name, EndpointBuiltIn};

    #[test]
    fn accepts_built_in_names_with_two_or_more_parts() {
        assert!(is_built_in_name(b"AWS::UseFIPS"));
        assert!(is_built_in_name(b"AWS::Auth::AccountIdEndpointMode"));
        assert_eq!(
            "SDK::Endpoint",
            EndpointBuiltIn::from_static("SDK::Endpoint").as_str()
        );
    }

    #[test]
    fn rejects_names_not_of_the_form_prefix_name() {
        for name in [
            "",
            "UseFIPS",
            "AWS::",
            "::UseFIPS",
            "AWS:UseFIPS",
            "AWS::::UseFIPS",
        ] {
            assert!(!is_built_in_name(name.as_bytes()), "{name}");
        }
    }

    #[test]
    fn new_takes_an_owned_name_and_checks_it() {
        let built_in = EndpointBuiltIn::new(String::from("Custom::Region")).unwrap();
        assert_eq!("Custom::Region", built_in.as_str());
        assert!(EndpointBuiltIn::new(String::from("Region")).is_err());
    }

    #[test]
    fn an_owned_name_equals_the_same_static_name() {
        let owned = EndpointBuiltIn::new(String::from("AWS::UseFIPS")).unwrap();
        assert_eq!(EndpointBuiltIn::USE_FIPS, owned);
    }

    #[test]
    #[should_panic(expected = "an endpoint built-in name must be of the form `Prefix::Name`")]
    fn from_static_panics_on_an_invalid_name() {
        let _ = EndpointBuiltIn::from_static("UseFIPS");
    }
}
