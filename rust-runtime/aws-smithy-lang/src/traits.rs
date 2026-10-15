/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Applied traits and typed trait codecs.
//!
//! The model always stores trait values as generic [`Node`]s. A [`TraitCodec`] decodes a
//! typed value on request without changing what the model stores, so a malformed value is
//! reported as an error rather than silently degrading to a dynamic trait. Downstream
//! crates can implement [`TraitCodec`] for their own traits without registering anything.

use crate::diagnostic::{fmt_located, SourceLocation};
use crate::node::{Node, NodeObject, NodeValue};
use crate::shape_id::ShapeId;
use std::fmt;

/// `smithy.api#documentation`
pub const DOCUMENTATION: ShapeId = ShapeId::from_static("smithy.api#documentation");
/// `smithy.api#required`
pub const REQUIRED: ShapeId = ShapeId::from_static("smithy.api#required");
/// `smithy.api#input`
pub const INPUT: ShapeId = ShapeId::from_static("smithy.api#input");
/// `smithy.api#output`
pub const OUTPUT: ShapeId = ShapeId::from_static("smithy.api#output");
/// `smithy.api#error`
pub const ERROR: ShapeId = ShapeId::from_static("smithy.api#error");
/// `smithy.api#trait`
pub const TRAIT: ShapeId = ShapeId::from_static("smithy.api#trait");
/// `smithy.api#mixin`
pub const MIXIN: ShapeId = ShapeId::from_static("smithy.api#mixin");
/// `smithy.api#enumValue`
pub const ENUM_VALUE: ShapeId = ShapeId::from_static("smithy.api#enumValue");
/// `smithy.api#private`
pub const PRIVATE: ShapeId = ShapeId::from_static("smithy.api#private");

/// Converts between a trait's generic [`Node`] value and a typed Rust value.
///
/// ```
/// use aws_smithy_lang::traits::{TraitCodec, TraitDecodeError};
/// use aws_smithy_lang::{Node, ShapeId};
///
/// struct Retries(u64);
///
/// impl TraitCodec for Retries {
///     const ID: ShapeId = ShapeId::from_static("com.example#retries");
///
///     fn from_node(value: &Node) -> Result<Self, TraitDecodeError> {
///         value
///             .as_number()
///             .and_then(|n| n.as_u64())
///             .map(Retries)
///             .ok_or_else(|| TraitDecodeError::at(value, "expected a non-negative integer"))
///     }
///
///     fn to_node(&self) -> Node {
///         Node::from(self.0)
///     }
/// }
/// ```
pub trait TraitCodec: Sized {
    /// The absolute shape ID of the trait.
    const ID: ShapeId;

    /// Decodes a trait value.
    ///
    /// Report problems with [`TraitDecodeError::at`] so the error points at the offending
    /// (possibly nested) value.
    fn from_node(value: &Node) -> Result<Self, TraitDecodeError>;

    /// Encodes this value as a trait node.
    fn to_node(&self) -> Node;
}

pub(crate) fn decode<T: TraitCodec>(value: Option<&Node>) -> Result<Option<T>, TraitDecodeError> {
    match value {
        None => Ok(None),
        Some(value) => T::from_node(value)
            .map(Some)
            .map_err(|err| err.for_trait(T::ID, value)),
    }
}

/// A trait value could not be decoded by a [`TraitCodec`].
///
/// The error records the trait ID and the [`SourceLocation`] of the offending value, so a
/// malformed nested value in a loaded model can be found by its line and JSON Pointer.
#[derive(Debug)]
pub struct TraitDecodeError {
    trait_id: Option<ShapeId>,
    message: String,
    location: SourceLocation,
    source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
}

impl TraitDecodeError {
    /// Creates an error with a message. The location defaults to the decoded trait value.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            trait_id: None,
            message: message.into(),
            location: SourceLocation::none(),
            source: None,
        }
    }

    /// Creates an error about `node`, which may be nested inside the trait value.
    pub fn at(node: &Node, message: impl Into<String>) -> Self {
        Self {
            location: node.source_location().clone(),
            ..Self::new(message)
        }
    }

    /// Creates an error with a message and an underlying cause.
    pub fn with_source(
        message: impl Into<String>,
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            source: Some(source.into()),
            ..Self::new(message)
        }
    }

    /// Sets the location of the offending value.
    pub fn with_source_location(mut self, location: SourceLocation) -> Self {
        self.location = location;
        self
    }

    fn for_trait(mut self, trait_id: ShapeId, value: &Node) -> Self {
        self.trait_id.get_or_insert(trait_id);
        if self.location.is_none() {
            self.location = value.source_location().clone();
        }
        self
    }

    /// The trait being decoded, when known.
    pub fn trait_id(&self) -> Option<&ShapeId> {
        self.trait_id.as_ref()
    }

    /// The error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Where the offending value was loaded from, or [`SourceLocation::none`] for values
    /// created in code.
    pub fn source_location(&self) -> &SourceLocation {
        &self.location
    }
}

impl fmt::Display for TraitDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.trait_id {
            Some(id) => fmt_located(
                f,
                &self.location,
                format_args!("invalid `{id}` trait value: {}", self.message),
            ),
            None => fmt_located(
                f,
                &self.location,
                format_args!("invalid trait value: {}", self.message),
            ),
        }
    }
}

impl std::error::Error for TraitDecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|e| e as &(dyn std::error::Error + 'static))
    }
}

fn expect_object(value: &Node) -> Result<&NodeObject, TraitDecodeError> {
    value
        .expect_object()
        .map_err(|e| TraitDecodeError::with_source(e.to_string(), e))
}

/// `smithy.api#` IDs are resolved for relative trait names such as `"required"`, matching Java
/// Smithy's `Trait` lookups.
pub(crate) fn find<'m>(
    map: &'m crate::shape::TraitMap,
    id: &str,
) -> Option<(&'m ShapeId, &'m Node)> {
    if id.contains('#') {
        map.get_key_value(id)
    } else {
        // Relative names resolve against the prelude without allocating.
        map.iter()
            .find(|(key, _)| key.name() == id && key.namespace() == crate::prelude::NAMESPACE)
    }
}

macro_rules! annotation_trait {
    ($(#[$doc:meta])* $name:ident, $id:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
        pub struct $name;

        impl TraitCodec for $name {
            const ID: ShapeId = $id;

            fn from_node(value: &Node) -> Result<Self, TraitDecodeError> {
                expect_object(value).map(|_| $name)
            }

            fn to_node(&self) -> Node {
                Node::from(NodeObject::new())
            }
        }
    };
}

annotation_trait!(
    /// `@required`
    Required,
    REQUIRED
);
annotation_trait!(
    /// `@input`
    Input,
    INPUT
);
annotation_trait!(
    /// `@output`
    Output,
    OUTPUT
);

/// `@documentation`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Documentation(pub String);

impl TraitCodec for Documentation {
    const ID: ShapeId = DOCUMENTATION;

    fn from_node(value: &Node) -> Result<Self, TraitDecodeError> {
        value
            .expect_str()
            .map(|s| Documentation(s.to_owned()))
            .map_err(|e| TraitDecodeError::with_source(e.to_string(), e))
    }

    fn to_node(&self) -> Node {
        Node::from(self.0.clone())
    }
}

/// The `@error` trait: whether the client or server is at fault.
///
/// Named for the trait's value rather than `Error`, which would shadow
/// [`std::error::Error`] in glob imports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorFault {
    /// `"client"`
    Client,
    /// `"server"`
    Server,
}

impl TraitCodec for ErrorFault {
    const ID: ShapeId = ERROR;

    fn from_node(value: &Node) -> Result<Self, TraitDecodeError> {
        match value.as_str() {
            Some("client") => Ok(ErrorFault::Client),
            Some("server") => Ok(ErrorFault::Server),
            _ => Err(TraitDecodeError::at(
                value,
                r#"expected the string "client" or "server""#,
            )),
        }
    }

    fn to_node(&self) -> Node {
        Node::from(match self {
            ErrorFault::Client => "client",
            ErrorFault::Server => "server",
        })
    }
}

/// `@trait`: marks a shape as a trait definition. The definition's properties are kept as-is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TraitDefinition(pub NodeObject);

impl TraitCodec for TraitDefinition {
    const ID: ShapeId = TRAIT;

    fn from_node(value: &Node) -> Result<Self, TraitDecodeError> {
        expect_object(value).map(|o| TraitDefinition(o.clone()))
    }

    fn to_node(&self) -> Node {
        Node::from(self.0.clone())
    }
}

/// `@enumValue`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnumValue {
    /// The value of an `enum` member.
    String(String),
    /// The value of an `intEnum` member.
    Int(i64),
}

impl TraitCodec for EnumValue {
    const ID: ShapeId = ENUM_VALUE;

    fn from_node(value: &Node) -> Result<Self, TraitDecodeError> {
        match value.value() {
            NodeValue::String(s) => Ok(EnumValue::String(s.clone())),
            NodeValue::Number(n) => n
                .as_i64()
                .map(EnumValue::Int)
                .ok_or_else(|| TraitDecodeError::at(value, "expected an integer that fits in i64")),
            _ => Err(TraitDecodeError::at(
                value,
                format!("expected a string or integer, found {}", value.type_name()),
            )),
        }
    }

    fn to_node(&self) -> Node {
        match self {
            EnumValue::String(s) => Node::from(s.clone()),
            EnumValue::Int(i) => Node::from(*i),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

    #[test]
    fn codecs_round_trip() {
        fn check<T: TraitCodec + PartialEq + fmt::Debug>(value: T) {
            assert_eq!(T::from_node(&value.to_node()).unwrap(), value);
        }
        check(Required);
        check(Input);
        check(Output);
        check(Documentation("hi".into()));
        check(ErrorFault::Client);
        check(ErrorFault::Server);
        check(TraitDefinition::default());
        check(EnumValue::String("A".into()));
        check(EnumValue::Int(-3));
    }

    #[test]
    fn malformed_values_are_errors() {
        let err = decode::<Documentation>(Some(&Node::from(true))).unwrap_err();
        assert_eq!(err.trait_id(), Some(&DOCUMENTATION));
        assert!(err.source_location().is_none());
        assert!(err.source().is_some());
        assert_eq!(
            err.to_string(),
            "invalid `smithy.api#documentation` trait value: expected string node but found boolean"
        );
        assert!(decode::<ErrorFault>(Some(&Node::from("neither"))).is_err());
        assert!(decode::<Required>(Some(&Node::null())).is_err());
        assert!(decode::<EnumValue>(Some(&Node::from(u64::MAX))).is_err());
        assert!(decode::<Required>(None).unwrap().is_none());
    }

    #[test]
    fn errors_point_at_the_offending_value() {
        let mut node = Node::from(vec![Node::from("x")]);
        node.assign_locations(
            &"m.json".into(),
            &mut String::from("/shapes/a#B/traits/a#t"),
            None,
        );
        // The codec reported no location: the trait value's location is used.
        let err = decode::<Documentation>(Some(&node)).unwrap_err();
        assert_eq!(
            err.source_location().pointer(),
            Some("/shapes/a#B/traits/a#t")
        );
        assert!(
            err.to_string().ends_with("(at /shapes/a#B/traits/a#t)"),
            "{err}"
        );
        // A codec can point at a nested value.
        #[derive(Debug)]
        struct First;
        impl TraitCodec for First {
            const ID: ShapeId = ShapeId::from_static("a#t");
            fn from_node(value: &Node) -> Result<Self, TraitDecodeError> {
                let first = &value.as_array().unwrap()[0];
                Err(TraitDecodeError::at(first, "bad element"))
            }
            fn to_node(&self) -> Node {
                Node::null()
            }
        }
        let err = decode::<First>(Some(&node)).unwrap_err();
        assert_eq!(
            err.source_location().pointer(),
            Some("/shapes/a#B/traits/a#t/0")
        );
        assert_eq!(err.trait_id().unwrap(), "a#t");
    }

    #[test]
    fn find_resolves_relative_prelude_names() {
        let mut map = crate::shape::TraitMap::new();
        map.insert(REQUIRED, Node::from(NodeObject::new()));
        map.insert(ShapeId::new("a#required").unwrap(), Node::null());
        assert_eq!(find(&map, "required").unwrap().0, &REQUIRED);
        assert_eq!(find(&map, "a#required").unwrap().0, "a#required");
        assert!(find(&map, "documentation").is_none());
    }

    #[test]
    fn error_is_send_sync() {
        fn assert<T: std::error::Error + Send + Sync + 'static>() {}
        assert::<TraitDecodeError>();
    }
}
