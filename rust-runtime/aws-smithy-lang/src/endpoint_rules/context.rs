/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Codecs for the traits that bind endpoint parameters to client configuration and operation
//! input.
//!
//! These types only describe the bindings a model declares. Supplying parameter values is up
//! to the client: see [the consumer contract](super#consumer-contract) for the precedence the
//! generated smithy-rs clients use.

use super::codec::{check_members, object, optional_string, parameter_value};
use super::language::{Location, ParameterType, ParameterValue};
use crate::diagnostic::SourceLocation;
use crate::node::{Node, NodeObject};
use crate::shape_id::ShapeId;
use crate::traits::{TraitCodec, TraitDecodeError};

/// `smithy.rules#contextParam`
pub const CONTEXT_PARAM: ShapeId = ShapeId::from_static("smithy.rules#contextParam");
/// `smithy.rules#staticContextParams`
pub const STATIC_CONTEXT_PARAMS: ShapeId = ShapeId::from_static("smithy.rules#staticContextParams");
/// `smithy.rules#operationContextParams`
pub const OPERATION_CONTEXT_PARAMS: ShapeId =
    ShapeId::from_static("smithy.rules#operationContextParams");
/// `smithy.rules#clientContextParams`
pub const CLIENT_CONTEXT_PARAMS: ShapeId = ShapeId::from_static("smithy.rules#clientContextParams");

type Result<T> = std::result::Result<T, TraitDecodeError>;

/// `@contextParam`: binds an operation input member to an endpoint parameter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextParam {
    name: String,
}

impl ContextParam {
    /// Binds a member to the parameter `name`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    /// The endpoint parameter name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl TraitCodec for ContextParam {
    const ID: ShapeId = CONTEXT_PARAM;

    fn from_node(value: &Node) -> Result<Self> {
        let members = object(value, "a contextParam object")?;
        check_members(value, members, "contextParam", &["name"])?;
        let name = required_string(value, members, "name")?;
        Ok(Self { name })
    }

    fn to_node(&self) -> Node {
        Node::from(NodeObject::from_iter([("name", self.name.clone())]))
    }
}

/// One `@staticContextParams` entry: a parameter set to a constant for an operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticContextParam {
    name: String,
    value: ParameterValue,
    location: Location,
}

impl StaticContextParam {
    /// The endpoint parameter name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The constant value.
    pub fn value(&self) -> &ParameterValue {
        &self.value
    }

    /// Where the entry was loaded from.
    pub fn source_location(&self) -> &SourceLocation {
        &self.location.0
    }
}

/// `@staticContextParams`: endpoint parameters set to constants for an operation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StaticContextParams {
    params: Box<[StaticContextParam]>,
}

impl StaticContextParams {
    /// The entries, in declaration order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &StaticContextParam> + '_ {
        self.params.iter()
    }

    /// The entry for parameter `name`.
    pub fn get(&self, name: &str) -> Option<&StaticContextParam> {
        self.params.iter().find(|p| p.name == name)
    }
}

impl TraitCodec for StaticContextParams {
    const ID: ShapeId = STATIC_CONTEXT_PARAMS;

    fn from_node(value: &Node) -> Result<Self> {
        let params = object(value, "a staticContextParams object")?
            .iter()
            .map(|(name, definition)| {
                let members = object(definition, "a static context parameter definition")?;
                check_members(definition, members, "static context parameter", &["value"])?;
                let constant = members.get("value").ok_or_else(|| {
                    TraitDecodeError::at(definition, "missing required member `value`")
                })?;
                let value = static_value(constant)?;
                Ok(StaticContextParam {
                    name: name.to_owned(),
                    value,
                    location: Location(definition.source_location().clone()),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            params: params.into_boxed_slice(),
        })
    }

    fn to_node(&self) -> Node {
        Node::from(
            self.params
                .iter()
                .map(|p| {
                    let definition = NodeObject::from_iter([("value", value_node(&p.value))]);
                    (p.name.clone(), Node::from(definition))
                })
                .collect::<NodeObject>(),
        )
    }
}

/// A static value is a string, a boolean, or an array of strings.
fn static_value(value: &Node) -> Result<ParameterValue> {
    let kind = match value.value() {
        crate::node::NodeValue::Bool(_) => ParameterType::Boolean,
        crate::node::NodeValue::Array(_) => ParameterType::StringArray,
        _ => ParameterType::String,
    };
    parameter_value(value, kind).map_err(|_| {
        TraitDecodeError::at(
            value,
            "a static context parameter value must be a string, a boolean, or an array of strings",
        )
    })
}

fn value_node(value: &ParameterValue) -> Node {
    match value {
        ParameterValue::String(s) => Node::from(s.clone()),
        ParameterValue::Boolean(b) => Node::from(*b),
        ParameterValue::StringArray(items) => {
            Node::from(items.iter().cloned().map(Node::from).collect::<Vec<_>>())
        }
    }
}

/// One `@operationContextParams` entry: a parameter bound to a JMESPath into operation input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationContextParam {
    name: String,
    path: String,
    location: Location,
}

impl OperationContextParam {
    /// The endpoint parameter name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The JMESPath expression selecting the value from the operation input. It is recorded
    /// as written; this crate does not parse or evaluate it.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Where the entry was loaded from.
    pub fn source_location(&self) -> &SourceLocation {
        &self.location.0
    }
}

/// `@operationContextParams`: endpoint parameters bound to paths into operation input.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OperationContextParams {
    params: Box<[OperationContextParam]>,
}

impl OperationContextParams {
    /// The entries, in declaration order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &OperationContextParam> + '_ {
        self.params.iter()
    }

    /// The entry for parameter `name`.
    pub fn get(&self, name: &str) -> Option<&OperationContextParam> {
        self.params.iter().find(|p| p.name == name)
    }
}

impl TraitCodec for OperationContextParams {
    const ID: ShapeId = OPERATION_CONTEXT_PARAMS;

    fn from_node(value: &Node) -> Result<Self> {
        let params = object(value, "an operationContextParams object")?
            .iter()
            .map(|(name, definition)| {
                let members = object(definition, "an operation context parameter definition")?;
                check_members(
                    definition,
                    members,
                    "operation context parameter",
                    &["path"],
                )?;
                Ok(OperationContextParam {
                    name: name.to_owned(),
                    path: required_string(definition, members, "path")?,
                    location: Location(definition.source_location().clone()),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            params: params.into_boxed_slice(),
        })
    }

    fn to_node(&self) -> Node {
        Node::from(
            self.params
                .iter()
                .map(|p| {
                    let definition = NodeObject::from_iter([("path", p.path.clone())]);
                    (p.name.clone(), Node::from(definition))
                })
                .collect::<NodeObject>(),
        )
    }
}

/// One `@clientContextParams` entry: a parameter exposed as client configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientContextParam {
    name: String,
    kind: ParameterType,
    documentation: Option<String>,
    location: Location,
}

impl ClientContextParam {
    /// The endpoint parameter name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The configuration type: [`ParameterType::String`] or [`ParameterType::Boolean`].
    pub fn kind(&self) -> ParameterType {
        self.kind
    }

    /// The documentation.
    pub fn documentation(&self) -> Option<&str> {
        self.documentation.as_deref()
    }

    /// Where the entry was loaded from.
    pub fn source_location(&self) -> &SourceLocation {
        &self.location.0
    }
}

/// `@clientContextParams`: endpoint parameters a service exposes as client configuration.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClientContextParams {
    params: Box<[ClientContextParam]>,
}

impl ClientContextParams {
    /// The entries, in declaration order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &ClientContextParam> + '_ {
        self.params.iter()
    }

    /// The entry for parameter `name`.
    pub fn get(&self, name: &str) -> Option<&ClientContextParam> {
        self.params.iter().find(|p| p.name == name)
    }
}

impl TraitCodec for ClientContextParams {
    const ID: ShapeId = CLIENT_CONTEXT_PARAMS;

    fn from_node(value: &Node) -> Result<Self> {
        let params = object(value, "a clientContextParams object")?
            .iter()
            .map(|(name, definition)| {
                let members = object(definition, "a client context parameter definition")?;
                check_members(
                    definition,
                    members,
                    "client context parameter",
                    &["type", "documentation"],
                )?;
                let kind_node = members.get("type").ok_or_else(|| {
                    TraitDecodeError::at(definition, "missing required member `type`")
                })?;
                let kind = match kind_node.as_str() {
                    Some("string") => ParameterType::String,
                    Some("boolean") => ParameterType::Boolean,
                    _ => {
                        return Err(TraitDecodeError::at(
                            kind_node,
                            "a client context parameter `type` must be `string` or `boolean`",
                        ))
                    }
                };
                Ok(ClientContextParam {
                    name: name.to_owned(),
                    kind,
                    documentation: optional_string(members, "documentation")?,
                    location: Location(definition.source_location().clone()),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            params: params.into_boxed_slice(),
        })
    }

    fn to_node(&self) -> Node {
        Node::from(
            self.params
                .iter()
                .map(|p| {
                    let mut definition = NodeObject::new();
                    definition.insert("type", p.kind.as_str());
                    if let Some(documentation) = &p.documentation {
                        definition.insert("documentation", documentation.clone());
                    }
                    (p.name.clone(), Node::from(definition))
                })
                .collect::<NodeObject>(),
        )
    }
}

fn required_string(value: &Node, members: &NodeObject, name: &str) -> Result<String> {
    optional_string(members, name)?
        .ok_or_else(|| TraitDecodeError::at(value, format!("missing required member `{name}`")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Debug;

    fn round_trip<T: TraitCodec + PartialEq + Debug>(value: Node) -> T {
        let decoded = T::from_node(&value).unwrap();
        assert_eq!(decoded.to_node(), value);
        assert_eq!(T::from_node(&decoded.to_node()).unwrap(), decoded);
        decoded
    }

    fn object(members: impl IntoIterator<Item = (&'static str, Node)>) -> Node {
        Node::from(members.into_iter().collect::<NodeObject>())
    }

    #[test]
    fn context_param() {
        let decoded: ContextParam = round_trip(object([("name", Node::from("Bucket"))]));
        assert_eq!(decoded, ContextParam::new("Bucket"));
        assert!(ContextParam::from_node(&object([])).is_err());
        assert!(ContextParam::from_node(&object([("name", Node::from(1i64))])).is_err());
        let extra = object([("name", Node::from("a")), ("x", Node::from(true))]);
        assert!(ContextParam::from_node(&extra)
            .unwrap_err()
            .message()
            .contains("unknown contextParam member `x`"));
    }

    #[test]
    fn static_context_params() {
        let value = |v: Node| object([("value", v)]);
        let decoded: StaticContextParams = round_trip(object([
            ("UseArnRegion", value(Node::from(true))),
            ("Name", value(Node::from("s"))),
            ("Keys", value(Node::from(vec![Node::from("a")]))),
        ]));
        let names: Vec<_> = decoded.iter().map(|p| p.name()).collect();
        assert_eq!(names, ["UseArnRegion", "Name", "Keys"]);
        assert_eq!(
            decoded.get("Keys").unwrap().value(),
            &ParameterValue::StringArray(vec!["a".into()])
        );
        for bad in [
            Node::from(1i64),
            Node::null(),
            Node::from(vec![Node::from(true)]),
        ] {
            let err = StaticContextParams::from_node(&object([("P", value(bad))])).unwrap_err();
            assert!(
                err.message().contains("string, a boolean, or an array"),
                "{err}"
            );
        }
        assert!(StaticContextParams::from_node(&object([("P", object([]))])).is_err());
    }

    #[test]
    fn operation_context_params() {
        let decoded: OperationContextParams = round_trip(object([(
            "ResourceArns",
            object([("path", Node::from("keys(RequestItems)"))]),
        )]));
        assert_eq!(
            decoded.get("ResourceArns").unwrap().path(),
            "keys(RequestItems)"
        );
        let bad = object([("P", object([("path", Node::from(false))]))]);
        assert!(OperationContextParams::from_node(&bad).is_err());
    }

    #[test]
    fn client_context_params() {
        let decoded: ClientContextParams = round_trip(object([
            (
                "Accelerate",
                object([
                    ("type", Node::from("boolean")),
                    ("documentation", Node::from("Enables acceleration.")),
                ]),
            ),
            ("Name", object([("type", Node::from("string"))])),
        ]));
        let accelerate = decoded.get("Accelerate").unwrap();
        assert_eq!(accelerate.kind(), ParameterType::Boolean);
        assert_eq!(accelerate.documentation(), Some("Enables acceleration."));
        assert_eq!(decoded.get("Name").unwrap().kind(), ParameterType::String);
        let bad = object([("P", object([("type", Node::from("stringArray"))]))]);
        assert!(ClientContextParams::from_node(&bad)
            .unwrap_err()
            .message()
            .contains("`string` or `boolean`"));
    }
}
