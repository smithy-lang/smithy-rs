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

use crate::node::{Node, NodeObject};
use std::fmt;

/// `smithy.api#documentation`
pub const DOCUMENTATION: &str = "smithy.api#documentation";
/// `smithy.api#required`
pub const REQUIRED: &str = "smithy.api#required";
/// `smithy.api#input`
pub const INPUT: &str = "smithy.api#input";
/// `smithy.api#output`
pub const OUTPUT: &str = "smithy.api#output";
/// `smithy.api#error`
pub const ERROR: &str = "smithy.api#error";
/// `smithy.api#trait`
pub const TRAIT: &str = "smithy.api#trait";
/// `smithy.api#mixin`
pub const MIXIN: &str = "smithy.api#mixin";
/// `smithy.api#enumValue`
pub const ENUM_VALUE: &str = "smithy.api#enumValue";

/// Converts between a trait's generic [`Node`] value and a typed Rust value.
pub trait TraitCodec: Sized {
    /// The absolute shape ID of the trait.
    const ID: &'static str;

    /// Decodes a trait value.
    fn from_node(value: &Node) -> Result<Self, TraitDecodeError>;

    /// Encodes this value as a trait node.
    fn to_node(&self) -> Node;
}

pub(crate) fn decode<T: TraitCodec>(value: Option<&Node>) -> Result<Option<T>, TraitDecodeError> {
    match value {
        None => Ok(None),
        Some(value) => T::from_node(value)
            .map(Some)
            .map_err(|err| err.for_trait(T::ID)),
    }
}

/// A trait value could not be decoded by a [`TraitCodec`].
#[derive(Debug)]
pub struct TraitDecodeError {
    trait_id: Option<&'static str>,
    message: String,
    source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
}

impl TraitDecodeError {
    /// Creates an error with a message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            trait_id: None,
            message: message.into(),
            source: None,
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

    fn for_trait(mut self, trait_id: &'static str) -> Self {
        self.trait_id.get_or_insert(trait_id);
        self
    }

    /// The trait being decoded, when known.
    pub fn trait_id(&self) -> Option<&str> {
        self.trait_id
    }

    /// The error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for TraitDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.trait_id {
            Some(id) => write!(f, "invalid `{id}` trait value: {}", self.message),
            None => write!(f, "invalid trait value: {}", self.message),
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

macro_rules! annotation_trait {
    ($(#[$doc:meta])* $name:ident, $id:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
        pub struct $name;

        impl TraitCodec for $name {
            const ID: &'static str = $id;

            fn from_node(value: &Node) -> Result<Self, TraitDecodeError> {
                expect_object(value).map(|_| $name)
            }

            fn to_node(&self) -> Node {
                Node::Object(NodeObject::new())
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
    const ID: &'static str = DOCUMENTATION;

    fn from_node(value: &Node) -> Result<Self, TraitDecodeError> {
        value
            .expect_str()
            .map(|s| Documentation(s.to_owned()))
            .map_err(|e| TraitDecodeError::with_source(e.to_string(), e))
    }

    fn to_node(&self) -> Node {
        Node::String(self.0.clone())
    }
}

/// `@error`: whether the client or server is at fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// `"client"`
    Client,
    /// `"server"`
    Server,
}

impl TraitCodec for Error {
    const ID: &'static str = ERROR;

    fn from_node(value: &Node) -> Result<Self, TraitDecodeError> {
        match value.as_str() {
            Some("client") => Ok(Error::Client),
            Some("server") => Ok(Error::Server),
            _ => Err(TraitDecodeError::new(
                r#"expected the string "client" or "server""#,
            )),
        }
    }

    fn to_node(&self) -> Node {
        Node::from(match self {
            Error::Client => "client",
            Error::Server => "server",
        })
    }
}

/// `@trait`: marks a shape as a trait definition. The definition's properties are kept as-is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TraitDefinition(pub NodeObject);

impl TraitCodec for TraitDefinition {
    const ID: &'static str = TRAIT;

    fn from_node(value: &Node) -> Result<Self, TraitDecodeError> {
        expect_object(value).map(|o| TraitDefinition(o.clone()))
    }

    fn to_node(&self) -> Node {
        Node::Object(self.0.clone())
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
    const ID: &'static str = ENUM_VALUE;

    fn from_node(value: &Node) -> Result<Self, TraitDecodeError> {
        match value {
            Node::String(s) => Ok(EnumValue::String(s.clone())),
            Node::Number(n) => n
                .as_i64()
                .map(EnumValue::Int)
                .ok_or_else(|| TraitDecodeError::new("expected an integer that fits in i64")),
            other => Err(TraitDecodeError::new(format!(
                "expected a string or integer, found {}",
                other.type_name()
            ))),
        }
    }

    fn to_node(&self) -> Node {
        match self {
            EnumValue::String(s) => Node::String(s.clone()),
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
        check(Error::Client);
        check(Error::Server);
        check(TraitDefinition::default());
        check(EnumValue::String("A".into()));
        check(EnumValue::Int(-3));
    }

    #[test]
    fn malformed_values_are_errors() {
        let err = decode::<Documentation>(Some(&Node::Bool(true))).unwrap_err();
        assert_eq!(err.trait_id(), Some(DOCUMENTATION));
        assert!(err.source().is_some());
        assert_eq!(
            err.to_string(),
            "invalid `smithy.api#documentation` trait value: expected string node but found boolean"
        );
        assert!(decode::<Error>(Some(&Node::from("neither"))).is_err());
        assert!(decode::<Required>(Some(&Node::Null)).is_err());
        assert!(decode::<EnumValue>(Some(&Node::from(u64::MAX))).is_err());
        assert!(decode::<Required>(None).unwrap().is_none());
    }

    #[test]
    fn error_is_send_sync() {
        fn assert<T: std::error::Error + Send + Sync + 'static>() {}
        assert::<TraitDecodeError>();
    }
}
