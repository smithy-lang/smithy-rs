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
/// `smithy.api#paginated`
pub const PAGINATED: ShapeId = ShapeId::from_static("smithy.api#paginated");

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

/// `@paginated`: the input and output members an operation uses to return its results across
/// several responses.
///
/// Every property is optional in the trait value. On a service, the values are defaults for
/// each operation in the service that has its own `@paginated` trait; combine the two with
/// [`with_defaults_from`](Self::with_defaults_from).
///
/// Decoding checks only the form of the value: each property, when present, must be a
/// non-empty string. Member names and paths are not resolved. Unknown properties are ignored,
/// because the reference implementation reports them only as warnings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Paginated {
    input_token: Option<String>,
    output_token: Option<String>,
    items: Option<String>,
    page_size: Option<String>,
}

impl Paginated {
    const INPUT_TOKEN: &'static str = "inputToken";
    const OUTPUT_TOKEN: &'static str = "outputToken";
    const ITEMS: &'static str = "items";
    const PAGE_SIZE: &'static str = "pageSize";

    /// The name of the input member that holds the continuation token.
    pub fn input_token(&self) -> Option<&str> {
        self.input_token.as_deref()
    }

    /// The path to the output member that holds the continuation token: member names
    /// separated by `.`.
    pub fn output_token(&self) -> Option<&str> {
        self.output_token.as_deref()
    }

    /// The path to the output member that holds the paginated results: member names separated
    /// by `.`.
    pub fn items(&self) -> Option<&str> {
        self.items.as_deref()
    }

    /// The name of the input member that limits the number of results in each response.
    pub fn page_size(&self) -> Option<&str> {
        self.page_size.as_deref()
    }

    /// Returns this value with each omitted property taken from `defaults`, typically the
    /// containing service's trait. Properties set on `self` always win. With `None`, returns a
    /// copy of `self`.
    pub fn with_defaults_from(&self, defaults: Option<&Self>) -> Self {
        let Some(defaults) = defaults else {
            return self.clone();
        };
        let pick = |own: &Option<String>, default: &Option<String>| {
            own.as_ref().or(default.as_ref()).cloned()
        };
        Self {
            input_token: pick(&self.input_token, &defaults.input_token),
            output_token: pick(&self.output_token, &defaults.output_token),
            items: pick(&self.items, &defaults.items),
            page_size: pick(&self.page_size, &defaults.page_size),
        }
    }
}

impl TraitCodec for Paginated {
    const ID: ShapeId = PAGINATED;

    fn from_node(value: &Node) -> Result<Self, TraitDecodeError> {
        let object = expect_object(value)?;
        let property = |name: &str| -> Result<Option<String>, TraitDecodeError> {
            let Some(node) = object.get(name) else {
                return Ok(None);
            };
            match node.as_str() {
                Some("") => Err(TraitDecodeError::at(
                    node,
                    format!("`{name}` must not be empty"),
                )),
                Some(s) => Ok(Some(s.to_owned())),
                None => Err(TraitDecodeError::at(
                    node,
                    format!(
                        "expected `{name}` to be a string, found {}",
                        node.type_name()
                    ),
                )),
            }
        };
        Ok(Paginated {
            input_token: property(Self::INPUT_TOKEN)?,
            output_token: property(Self::OUTPUT_TOKEN)?,
            items: property(Self::ITEMS)?,
            page_size: property(Self::PAGE_SIZE)?,
        })
    }

    fn to_node(&self) -> Node {
        // The prelude's declaration order, which the reference implementation also uses.
        let object: NodeObject = [
            (Self::INPUT_TOKEN, &self.input_token),
            (Self::OUTPUT_TOKEN, &self.output_token),
            (Self::ITEMS, &self.items),
            (Self::PAGE_SIZE, &self.page_size),
        ]
        .into_iter()
        .filter_map(|(name, value)| Some((name, value.clone()?)))
        .collect();
        Node::from(object)
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
        check(Paginated::default());
        check(paginated(
            Some("token"),
            Some("result.next"),
            Some("result.items"),
            Some("size"),
        ));
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
    fn error_is_send_sync() {
        fn assert<T: std::error::Error + Send + Sync + 'static>() {}
        assert::<TraitDecodeError>();
    }

    fn paginated(
        input_token: Option<&str>,
        output_token: Option<&str>,
        items: Option<&str>,
        page_size: Option<&str>,
    ) -> Paginated {
        Paginated {
            input_token: input_token.map(str::to_owned),
            output_token: output_token.map(str::to_owned),
            items: items.map(str::to_owned),
            page_size: page_size.map(str::to_owned),
        }
    }

    /// Parses `json` as a `@paginated` value located at `/t` in `m.json`.
    fn paginated_node(json: &str) -> Node {
        let mut node = crate::node::parse_node(json).unwrap();
        node.assign_locations(&"m.json".into(), &mut String::from("/t"), None);
        node
    }

    #[test]
    fn paginated_decodes_present_properties_and_ignores_unknown_ones() {
        let value = paginated_node(
            r#"{"inputToken": "token", "outputToken": "result.next",
                "items": "result.items", "pageSize": "size", "future": 1}"#,
        );
        let decoded = decode::<Paginated>(Some(&value)).unwrap().unwrap();
        assert_eq!(decoded.input_token(), Some("token"));
        assert_eq!(decoded.output_token(), Some("result.next"));
        assert_eq!(decoded.items(), Some("result.items"));
        assert_eq!(decoded.page_size(), Some("size"));

        // A service's trait often sets only some properties; an operation's may set none.
        let decoded = decode::<Paginated>(Some(&paginated_node(r#"{"pageSize": "size"}"#)))
            .unwrap()
            .unwrap();
        assert_eq!(decoded, paginated(None, None, None, Some("size")));
        let decoded = decode::<Paginated>(Some(&paginated_node("{}")))
            .unwrap()
            .unwrap();
        assert_eq!(decoded, Paginated::default());
    }

    #[test]
    fn paginated_rejects_malformed_values_at_the_offending_property() {
        let err = decode::<Paginated>(Some(&paginated_node(r#""token""#))).unwrap_err();
        assert_eq!(err.trait_id(), Some(&PAGINATED));
        assert_eq!(err.source_location().pointer(), Some("/t"));
        assert!(err.source().is_some());

        for (json, pointer, message) in [
            (
                r#"{"inputToken": 1}"#,
                "/t/inputToken",
                "expected `inputToken` to be a string, found number",
            ),
            (
                r#"{"outputToken": null}"#,
                "/t/outputToken",
                "expected `outputToken` to be a string, found null",
            ),
            (
                r#"{"items": ["a"]}"#,
                "/t/items",
                "expected `items` to be a string, found array",
            ),
            (
                r#"{"inputToken": "token", "pageSize": ""}"#,
                "/t/pageSize",
                "`pageSize` must not be empty",
            ),
        ] {
            let err = decode::<Paginated>(Some(&paginated_node(json))).unwrap_err();
            assert_eq!(err.trait_id(), Some(&PAGINATED), "{json}");
            assert_eq!(err.source_location().pointer(), Some(pointer), "{json}");
            assert_eq!(err.message(), message, "{json}");
        }
    }

    #[test]
    fn paginated_encodes_in_prelude_declaration_order() {
        let node = paginated(Some("in"), Some("out"), Some("items"), Some("size")).to_node();
        let keys: Vec<_> = node.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["inputToken", "outputToken", "items", "pageSize"]);
        let node = paginated(None, Some("out"), None, None).to_node();
        let keys: Vec<_> = node.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["outputToken"]);
    }

    #[test]
    fn paginated_operation_values_override_service_defaults() {
        let service = paginated(
            Some("serviceIn"),
            Some("serviceOut"),
            Some("serviceItems"),
            Some("serviceSize"),
        );
        // Each property is chosen independently.
        let operation = paginated(Some("in"), None, Some("items"), None);
        assert_eq!(
            operation.with_defaults_from(Some(&service)),
            paginated(
                Some("in"),
                Some("serviceOut"),
                Some("items"),
                Some("serviceSize")
            )
        );
        assert_eq!(
            Paginated::default().with_defaults_from(Some(&service)),
            service
        );
        assert_eq!(service.with_defaults_from(Some(&operation)), service);
        assert_eq!(operation.with_defaults_from(None), operation);
    }
}
