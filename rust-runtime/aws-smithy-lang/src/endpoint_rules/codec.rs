/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Decoding and encoding of `smithy.rules#endpointBdd` trait values.

use super::bdd::{wire_byte_len, Bdd};
use super::language::*;
use super::{EndpointBdd, EndpointRulesLimits};
use crate::node::{Node, NodeObject, NodeValue};
use crate::traits::TraitDecodeError;

type Result<T> = std::result::Result<T, TraitDecodeError>;

const ROOT_MEMBERS: [&str; 7] = [
    "version",
    "parameters",
    "conditions",
    "results",
    "root",
    "nodeCount",
    "nodes",
];

pub(super) fn decode(value: &Node, limits: &EndpointRulesLimits) -> Result<EndpointBdd> {
    let root_object = object(value, "an endpointBdd object")?;
    let [version, parameters, conditions, results, root, node_count, nodes] =
        ROOT_MEMBERS.map(|name| root_object.get(name).ok_or(name));
    fn required<'a>(value: &Node, found: std::result::Result<&'a Node, &str>) -> Result<&'a Node> {
        found.map_err(|name| {
            TraitDecodeError::at(value, format!("missing required member `{name}`"))
        })
    }
    let member = |found| required(value, found);

    let version_node = member(version)?;
    let version = decode_version(version_node)?;
    if version < RulesVersion::V1_1 {
        return Err(TraitDecodeError::at(
            version_node,
            format!("endpointBdd requires rules engine version 1.1 or later, found {version}"),
        ));
    }

    let mut decoder = Decoder {
        limits,
        expression_nodes: 0,
        template_segments: 0,
    };

    let parameters_node = member(parameters)?;
    let parameters_object = object(parameters_node, "a `parameters` object")?;
    check_limit(
        parameters_node,
        "parameters",
        parameters_object.len(),
        limits.max_parameters(),
    )?;
    let parameters = parameters_object
        .iter()
        .map(|(name, value)| decoder.parameter(name, value))
        .collect::<Result<Vec<_>>>()?;

    let conditions_node = member(conditions)?;
    let conditions = array(conditions_node, "a `conditions` array")?;
    check_limit(
        conditions_node,
        "conditions",
        conditions.len(),
        limits.max_conditions(),
    )?;
    let conditions = conditions
        .iter()
        .map(|condition| decoder.condition(condition))
        .collect::<Result<Vec<_>>>()?;

    let results_node = member(results)?;
    let results = array(results_node, "a `results` array")?;
    check_limit(results_node, "results", results.len(), limits.max_results())?;
    let results = results
        .iter()
        .map(|result| decoder.result(result))
        .collect::<Result<Vec<_>>>()?;

    let root_node = member(root)?;
    let root = int32(root_node, "`root`")?;
    let node_count_node = member(node_count)?;
    let node_count = int32(node_count_node, "`nodeCount`")?;
    let node_count = usize::try_from(node_count)
        .ok()
        .filter(|count| *count >= 1)
        .ok_or_else(|| {
            TraitDecodeError::at(
                node_count_node,
                "`nodeCount` must be at least 1 to include the terminal sentinel",
            )
        })?;
    check_limit(node_count_node, "nodeCount", node_count, limits.max_nodes())?;

    let nodes_node = member(nodes)?;
    let bytes = decode_base64(nodes_node, node_count)?;
    let bdd = Bdd::from_wire_bytes(
        root,
        node_count,
        &bytes,
        conditions.len(),
        results.len() + 1,
    )
    .map_err(|err| {
        let at = if err.is_at_root() {
            root_node
        } else {
            nodes_node
        };
        TraitDecodeError::at(at, err.message().to_owned())
    })?;

    Ok(EndpointBdd {
        version,
        parameters: Parameters::new(parameters),
        conditions: conditions.into_boxed_slice(),
        results: results.into_boxed_slice(),
        bdd,
    })
}

/// Validates the encoded length before allocating, then decodes the standard base64 payload.
fn decode_base64(value: &Node, node_count: usize) -> Result<Vec<u8>> {
    let text = value
        .as_str()
        .ok_or_else(|| TraitDecodeError::at(value, "`nodes` must be a base64 string"))?;
    let decoded_len = wire_byte_len(node_count)
        .ok_or_else(|| TraitDecodeError::at(value, "`nodeCount` is too large"))?;
    let encoded_len = decoded_len
        .checked_add(2)
        .map(|n| n / 3)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| TraitDecodeError::at(value, "`nodeCount` is too large"))?;
    if text.len() != encoded_len {
        return Err(TraitDecodeError::at(
            value,
            format!(
                "`nodes` has {} base64 characters, but `nodeCount` {node_count} requires {encoded_len}",
                text.len()
            ),
        ));
    }
    let bytes = base64_simd::STANDARD.decode_to_vec(text).map_err(|err| {
        TraitDecodeError::with_source("`nodes` is not valid padded standard base64", err)
            .with_source_location(value.source_location().clone())
    })?;
    if bytes.len() != decoded_len {
        return Err(TraitDecodeError::at(
            value,
            format!(
                "`nodes` decodes to {} bytes, but `nodeCount` {node_count} requires {decoded_len}",
                bytes.len()
            ),
        ));
    }
    Ok(bytes)
}

struct Decoder<'a> {
    limits: &'a EndpointRulesLimits,
    expression_nodes: usize,
    template_segments: usize,
}

impl Decoder<'_> {
    fn parameter(&mut self, name: &str, value: &Node) -> Result<Parameter> {
        if name.is_empty() {
            return Err(TraitDecodeError::at(
                value,
                "parameter names must not be empty",
            ));
        }
        let object = object(value, "a parameter object")?;
        check_members(
            value,
            object,
            "parameter",
            &[
                "type",
                "builtIn",
                "required",
                "default",
                "documentation",
                "deprecated",
            ],
        )?;
        let kind_node = object.get("type").ok_or_else(|| {
            TraitDecodeError::at(value, format!("parameter `{name}` is missing `type`"))
        })?;
        let kind = match kind_node.as_str() {
            Some(s) if s.eq_ignore_ascii_case("string") => ParameterType::String,
            Some(s) if s.eq_ignore_ascii_case("boolean") => ParameterType::Boolean,
            Some(s) if s.eq_ignore_ascii_case("stringArray") => ParameterType::StringArray,
            _ => {
                return Err(TraitDecodeError::at(
                    kind_node,
                    "parameter `type` must be `string`, `boolean`, or `stringArray`",
                ))
            }
        };
        let required = match object.get("required") {
            None => false,
            Some(node) => node
                .as_bool()
                .ok_or_else(|| TraitDecodeError::at(node, "`required` must be a boolean"))?,
        };
        let default = match object.get("default") {
            None => None,
            Some(node) => {
                let default = parameter_value(node, kind)?;
                if !required {
                    return Err(TraitDecodeError::at(
                        node,
                        format!("parameter `{name}` has a default but is not required"),
                    ));
                }
                Some(default)
            }
        };
        let deprecated = match object.get("deprecated") {
            None => None,
            Some(node) => {
                let deprecated = self::object(node, "a `deprecated` object")?;
                check_members(node, deprecated, "deprecated", &["message", "since"])?;
                Some(Deprecated {
                    message: optional_string(deprecated, "message")?,
                    since: optional_string(deprecated, "since")?,
                })
            }
        };
        Ok(Parameter {
            name: name.to_owned(),
            kind,
            required,
            default,
            built_in: optional_string(object, "builtIn")?,
            documentation: optional_string(object, "documentation")?,
            deprecated,
            location: Location(value.source_location().clone()),
        })
    }

    fn condition(&mut self, value: &Node) -> Result<Condition> {
        let object = object(value, "a condition object")?;
        check_members(value, object, "condition", &["fn", "argv", "assign"])?;
        let function = self.function(value, object, 0)?;
        let assign = optional_string(object, "assign")?;
        if assign.as_deref() == Some("") {
            return Err(TraitDecodeError::at(
                object.get("assign").unwrap_or(value),
                "`assign` must not be empty",
            ));
        }
        Ok(Condition {
            function,
            assign,
            location: Location(value.source_location().clone()),
        })
    }

    fn result(&mut self, value: &Node) -> Result<ResultRule> {
        let object = object(value, "a result rule object")?;
        check_members(
            value,
            object,
            "result rule",
            &["type", "conditions", "documentation", "endpoint", "error"],
        )?;
        if let Some(conditions) = object.get("conditions") {
            if !array(conditions, "a `conditions` array")?.is_empty() {
                return Err(TraitDecodeError::at(
                    conditions,
                    "endpointBdd results must have no conditions",
                ));
            }
        }
        let kind_node = object
            .get("type")
            .ok_or_else(|| TraitDecodeError::at(value, "result rule is missing `type`"))?;
        let body = |name: &str, other: &str| {
            if let Some(found) = object.get(other) {
                return Err(TraitDecodeError::at(
                    found,
                    format!("a `{name}` rule must not have `{other}`"),
                ));
            }
            object.get(name).ok_or_else(|| {
                TraitDecodeError::at(value, format!("`{name}` rule is missing `{name}`"))
            })
        };
        let kind = match kind_node.as_str() {
            Some("endpoint") => ResultKind::Endpoint(self.endpoint(body("endpoint", "error")?)?),
            Some("error") => ResultKind::Error(self.expression(body("error", "endpoint")?, 0)?),
            Some("tree") => {
                return Err(TraitDecodeError::at(
                    kind_node,
                    "tree rules are not allowed in endpointBdd results",
                ))
            }
            _ => {
                return Err(TraitDecodeError::at(
                    kind_node,
                    "result rule `type` must be `endpoint` or `error`",
                ))
            }
        };
        Ok(ResultRule {
            documentation: optional_string(object, "documentation")?,
            kind,
            location: Location(value.source_location().clone()),
        })
    }

    fn endpoint(&mut self, value: &Node) -> Result<EndpointDefinition> {
        let object = object(value, "an endpoint object")?;
        check_members(value, object, "endpoint", &["url", "properties", "headers"])?;
        let url = object
            .get("url")
            .ok_or_else(|| TraitDecodeError::at(value, "endpoint is missing `url`"))?;
        let url = self.expression(url, 0)?;
        let properties = match object.get("properties") {
            None => Vec::new(),
            Some(node) => self::object(node, "a `properties` object")?
                .iter()
                .map(|(name, value)| Ok((name.to_owned(), self.literal(value, 0)?)))
                .collect::<Result<_>>()?,
        };
        let headers = match object.get("headers") {
            None => Vec::new(),
            Some(node) => self::object(node, "a `headers` object")?
                .iter()
                .map(|(name, values)| {
                    let values = array(values, "an array of header values")?
                        .iter()
                        .map(|value| self.expression(value, 0))
                        .collect::<Result<Vec<_>>>()?;
                    Ok((name.to_owned(), values.into_boxed_slice()))
                })
                .collect::<Result<_>>()?,
        };
        Ok(EndpointDefinition {
            url,
            headers: headers.into_boxed_slice(),
            properties: properties.into_boxed_slice(),
            location: Location(value.source_location().clone()),
        })
    }

    /// Counts one expression or literal at `depth`, enforcing limits.
    fn enter(&mut self, value: &Node, depth: usize) -> Result<()> {
        if depth >= self.limits.max_expression_depth() {
            return Err(TraitDecodeError::at(
                value,
                format!(
                    "expression nesting exceeds the limit of {}",
                    self.limits.max_expression_depth()
                ),
            ));
        }
        self.expression_nodes += 1;
        if self.expression_nodes > self.limits.max_expression_nodes() {
            return Err(TraitDecodeError::at(
                value,
                format!(
                    "the trait exceeds the limit of {} expressions",
                    self.limits.max_expression_nodes()
                ),
            ));
        }
        Ok(())
    }

    fn expression(&mut self, value: &Node, depth: usize) -> Result<Expression> {
        let Some(object) = value.as_object() else {
            return self.literal_expression(value, depth);
        };
        match (object.get("ref"), object.get("fn")) {
            (Some(_), Some(_)) => Err(TraitDecodeError::at(
                value,
                "an expression must not have both `ref` and `fn`",
            )),
            (Some(reference), None) => {
                self.enter(value, depth)?;
                check_members(value, object, "reference", &["ref"])?;
                let name = reference
                    .as_str()
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| {
                        TraitDecodeError::at(reference, "`ref` must be a non-empty string")
                    })?;
                Ok(Expression {
                    kind: ExpressionKind::Reference(name.to_owned()),
                    location: Location(value.source_location().clone()),
                })
            }
            (None, Some(_)) => {
                check_members(value, object, "function", &["fn", "argv"])?;
                self.function(value, object, depth)
            }
            (None, None) => self.literal_expression(value, depth),
        }
    }

    fn literal_expression(&mut self, value: &Node, depth: usize) -> Result<Expression> {
        Ok(Expression {
            kind: ExpressionKind::Literal(self.literal(value, depth)?),
            location: Location(value.source_location().clone()),
        })
    }

    /// Decodes `fn` and `argv` from `object`, which is `value`'s object.
    fn function(&mut self, value: &Node, object: &NodeObject, depth: usize) -> Result<Expression> {
        self.enter(value, depth)?;
        let name = object
            .get("fn")
            .ok_or_else(|| TraitDecodeError::at(value, "missing `fn`"))?;
        let name = name
            .as_str()
            .filter(|name| !name.is_empty())
            .ok_or_else(|| TraitDecodeError::at(name, "`fn` must be a non-empty string"))?;
        let argv = object
            .get("argv")
            .ok_or_else(|| TraitDecodeError::at(value, "missing `argv`"))?;
        let arguments = array(argv, "an `argv` array")?
            .iter()
            .map(|argument| self.expression(argument, depth + 1))
            .collect::<Result<Vec<_>>>()?;
        if name == "getAttr" {
            if let Some(path) = arguments.get(1) {
                if let ExpressionKind::Literal(Literal::String(template)) = &path.kind {
                    if let Some(text) = template.as_static() {
                        parse_path(text).map_err(|message| {
                            TraitDecodeError::at(&argv.as_array().expect("array")[1], message)
                        })?;
                    }
                }
            }
        }
        Ok(Expression {
            kind: ExpressionKind::Function {
                name: name.to_owned(),
                arguments: arguments.into_boxed_slice(),
            },
            location: Location(value.source_location().clone()),
        })
    }

    fn literal(&mut self, value: &Node, depth: usize) -> Result<Literal> {
        self.enter(value, depth)?;
        Ok(match value.value() {
            NodeValue::Bool(b) => Literal::Boolean(*b),
            NodeValue::Number(n) => {
                Literal::Integer(n.as_i64().and_then(|n| i32::try_from(n).ok()).ok_or_else(
                    || {
                        TraitDecodeError::at(
                            value,
                            "integer literals must be integers that fit in a signed 32-bit integer",
                        )
                    },
                )?)
            }
            NodeValue::String(text) => {
                let template = parse_template(text).map_err(|m| TraitDecodeError::at(value, m))?;
                self.template_segments += template.segments.len();
                if self.template_segments > self.limits.max_template_segments() {
                    return Err(TraitDecodeError::at(
                        value,
                        format!(
                            "the trait exceeds the limit of {} template segments",
                            self.limits.max_template_segments()
                        ),
                    ));
                }
                Literal::String(template)
            }
            NodeValue::Array(items) => Literal::Tuple(
                items
                    .iter()
                    .map(|item| self.literal(item, depth + 1))
                    .collect::<Result<_>>()?,
            ),
            NodeValue::Object(members) => Literal::Record(
                members
                    .iter()
                    .map(|(key, item)| Ok((key.to_owned(), self.literal(item, depth + 1)?)))
                    .collect::<Result<_>>()?,
            ),
            NodeValue::Null => {
                return Err(TraitDecodeError::at(value, "literals must not be null"))
            }
        })
    }
}

fn decode_version(value: &Node) -> Result<RulesVersion> {
    value.as_str().and_then(RulesVersion::parse).ok_or_else(|| {
        TraitDecodeError::at(
            value,
            "`version` must be a string such as \"1.1\" with at least two numeric components",
        )
    })
}

pub(super) fn parameter_value(value: &Node, kind: ParameterType) -> Result<ParameterValue> {
    let decoded = match (kind, value.value()) {
        (ParameterType::String, NodeValue::String(s)) => Some(ParameterValue::String(s.clone())),
        (ParameterType::Boolean, NodeValue::Bool(b)) => Some(ParameterValue::Boolean(*b)),
        (ParameterType::StringArray, NodeValue::Array(items)) => items
            .iter()
            .map(|item| item.as_str().map(str::to_owned))
            .collect::<Option<_>>()
            .map(ParameterValue::StringArray),
        _ => None,
    };
    decoded.ok_or_else(|| {
        TraitDecodeError::at(
            value,
            format!("`default` must be a {} value", kind.as_str()),
        )
    })
}

/// Parses a template string, where `{{` and `}}` escape braces and `{name}` or
/// `{name#path}` is a reference.
pub(crate) fn parse_template(text: &str) -> std::result::Result<Template, String> {
    let mut segments = Vec::new();
    let mut literal = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(['{', '}']) {
        literal.push_str(&rest[..at]);
        let brace = rest.as_bytes()[at];
        let after = &rest[at + 1..];
        if after.as_bytes().first() == Some(&brace) {
            literal.push(brace as char);
            rest = &after[1..];
            continue;
        }
        if brace == b'}' {
            return Err("unmatched `}` in template; write `}}` for a literal brace".into());
        }
        let close = after
            .find('}')
            .ok_or("unclosed `{` in template; write `{{` for a literal brace")?;
        let reference = &after[..close];
        if reference.contains('{') {
            return Err("template references must not contain `{`".into());
        }
        let (name, path) = match reference.split_once('#') {
            Some((name, path)) => (name, parse_path(path)?),
            None => (reference, Vec::new()),
        };
        if name.is_empty() {
            return Err("template references must name a parameter or assignment".into());
        }
        if !literal.is_empty() {
            segments.push(TemplateSegment::Literal(std::mem::take(&mut literal)));
        }
        segments.push(TemplateSegment::Reference {
            name: name.to_owned(),
            path: path.into_boxed_slice(),
        });
        rest = &after[close + 1..];
    }
    literal.push_str(rest);
    if !literal.is_empty() {
        segments.push(TemplateSegment::Literal(literal));
    }
    Ok(Template {
        segments: segments.into_boxed_slice(),
    })
}

/// Parses an attribute path such as `a.b[0]`. Only the first part may omit its key, as in
/// `[0]`. Indexes must be written canonically so paths re-encode exactly.
pub(crate) fn parse_path(text: &str) -> std::result::Result<Vec<PathSegment>, String> {
    let invalid = || format!("invalid attribute path `{text}`");
    let mut segments = Vec::new();
    for (position, part) in text.split('.').enumerate() {
        let (key, index) = match part.find('[') {
            Some(open) => {
                let index = part[open..]
                    .strip_prefix('[')
                    .and_then(|s| s.strip_suffix(']'))
                    .ok_or_else(invalid)?;
                let parsed = index.parse::<i32>().map_err(|_| invalid())?;
                if parsed.to_string() != index {
                    return Err(invalid());
                }
                (&part[..open], Some(parsed))
            }
            None => (part, None),
        };
        if key.contains(']') {
            return Err(invalid());
        }
        if key.is_empty() {
            if position > 0 || index.is_none() {
                return Err(invalid());
            }
        } else {
            segments.push(PathSegment::Key(key.to_owned()));
        }
        segments.extend(index.map(PathSegment::Index));
    }
    Ok(segments)
}

pub(super) fn object<'a>(value: &'a Node, expected: &str) -> Result<&'a NodeObject> {
    value.as_object().ok_or_else(|| {
        TraitDecodeError::at(
            value,
            format!("expected {expected}, found {}", value.type_name()),
        )
    })
}

pub(super) fn array<'a>(value: &'a Node, expected: &str) -> Result<&'a [Node]> {
    value.as_array().ok_or_else(|| {
        TraitDecodeError::at(
            value,
            format!("expected {expected}, found {}", value.type_name()),
        )
    })
}

pub(super) fn optional_string(object: &NodeObject, name: &str) -> Result<Option<String>> {
    object
        .get(name)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| TraitDecodeError::at(value, format!("`{name}` must be a string")))
        })
        .transpose()
}

pub(super) fn check_members(
    value: &Node,
    object: &NodeObject,
    what: &str,
    allowed: &[&str],
) -> Result<()> {
    match object.keys().find(|key| !allowed.contains(key)) {
        Some(key) => Err(TraitDecodeError::at(
            object.get(key).unwrap_or(value),
            format!("unknown {what} member `{key}`"),
        )),
        None => Ok(()),
    }
}

fn check_limit(value: &Node, what: &str, count: usize, limit: usize) -> Result<()> {
    if count > limit {
        return Err(TraitDecodeError::at(
            value,
            format!("`{what}` has {count} entries, exceeding the limit of {limit}"),
        ));
    }
    Ok(())
}

/// An integral JSON number in the signed 32-bit range. Unlike the reference implementation,
/// fractional and out-of-range values are rejected rather than truncated.
fn int32(value: &Node, what: &str) -> Result<i32> {
    value
        .as_number()
        .and_then(|n| n.as_i64())
        .and_then(|n| i32::try_from(n).ok())
        .ok_or_else(|| {
            TraitDecodeError::at(value, format!("{what} must be a signed 32-bit integer"))
        })
}

pub(super) fn encode(rules: &EndpointBdd) -> Node {
    let mut object = NodeObject::new();
    object.insert("version", rules.version.to_string());
    object.insert(
        "parameters",
        rules
            .parameters
            .iter()
            .map(|parameter| (parameter.name.clone(), encode_parameter(parameter)))
            .collect::<NodeObject>(),
    );
    object.insert(
        "conditions",
        Node::from(
            rules
                .conditions
                .iter()
                .map(encode_condition)
                .collect::<Vec<_>>(),
        ),
    );
    object.insert(
        "results",
        Node::from(rules.results.iter().map(encode_result).collect::<Vec<_>>()),
    );
    object.insert("root", i64::from(rules.bdd.encoded_root()));
    object.insert("nodeCount", rules.bdd.wire_node_count() as u64);
    object.insert(
        "nodes",
        base64_simd::STANDARD.encode_to_string(rules.bdd.to_wire_bytes()),
    );
    Node::from(object)
}

fn encode_parameter(parameter: &Parameter) -> Node {
    let mut object = NodeObject::new();
    object.insert("type", parameter.kind.as_str());
    if let Some(built_in) = &parameter.built_in {
        object.insert("builtIn", built_in.clone());
    }
    object.insert("required", parameter.required);
    if let Some(default) = &parameter.default {
        object.insert(
            "default",
            match default {
                ParameterValue::String(s) => Node::from(s.clone()),
                ParameterValue::Boolean(b) => Node::from(*b),
                ParameterValue::StringArray(items) => {
                    Node::from(items.iter().cloned().map(Node::from).collect::<Vec<_>>())
                }
            },
        );
    }
    if let Some(documentation) = &parameter.documentation {
        object.insert("documentation", documentation.clone());
    }
    if let Some(deprecated) = &parameter.deprecated {
        let mut members = NodeObject::new();
        if let Some(message) = &deprecated.message {
            members.insert("message", message.clone());
        }
        if let Some(since) = &deprecated.since {
            members.insert("since", since.clone());
        }
        object.insert("deprecated", members);
    }
    Node::from(object)
}

fn encode_condition(condition: &Condition) -> Node {
    let mut object = match encode_expression(&condition.function).into_value() {
        NodeValue::Object(object) => object,
        _ => unreachable!("conditions are function calls"),
    };
    if let Some(assign) = &condition.assign {
        object.insert("assign", assign.clone());
    }
    Node::from(object)
}

fn encode_result(result: &ResultRule) -> Node {
    let mut object = NodeObject::new();
    object.insert("conditions", Vec::<Node>::new());
    if let Some(documentation) = &result.documentation {
        object.insert("documentation", documentation.clone());
    }
    match &result.kind {
        ResultKind::Endpoint(endpoint) => {
            object.insert("type", "endpoint");
            object.insert("endpoint", encode_endpoint(endpoint));
        }
        ResultKind::Error(error) => {
            object.insert("type", "error");
            object.insert("error", encode_expression(error));
        }
    }
    Node::from(object)
}

fn encode_endpoint(endpoint: &EndpointDefinition) -> Node {
    let mut object = NodeObject::new();
    object.insert("url", encode_expression(&endpoint.url));
    object.insert(
        "properties",
        endpoint
            .properties
            .iter()
            .map(|(name, value)| (name.clone(), encode_literal(value)))
            .collect::<NodeObject>(),
    );
    object.insert(
        "headers",
        endpoint
            .headers
            .iter()
            .map(|(name, values)| {
                let values: Vec<_> = values.iter().map(encode_expression).collect();
                (name.clone(), Node::from(values))
            })
            .collect::<NodeObject>(),
    );
    Node::from(object)
}

fn encode_expression(expression: &Expression) -> Node {
    match &expression.kind {
        ExpressionKind::Literal(literal) => encode_literal(literal),
        ExpressionKind::Reference(name) => {
            Node::from(NodeObject::from_iter([("ref", name.clone())]))
        }
        ExpressionKind::Function { name, arguments } => {
            let mut object = NodeObject::new();
            object.insert("fn", name.clone());
            object.insert(
                "argv",
                Node::from(arguments.iter().map(encode_expression).collect::<Vec<_>>()),
            );
            Node::from(object)
        }
    }
}

fn encode_literal(literal: &Literal) -> Node {
    match literal {
        Literal::Boolean(b) => Node::from(*b),
        Literal::Integer(i) => Node::from(i64::from(*i)),
        Literal::String(template) => Node::from(encode_template(template)),
        Literal::Record(members) => Node::from(
            members
                .iter()
                .map(|(key, value)| (key.clone(), encode_literal(value)))
                .collect::<NodeObject>(),
        ),
        Literal::Tuple(items) => Node::from(items.iter().map(encode_literal).collect::<Vec<_>>()),
    }
}

pub(crate) fn encode_template(template: &Template) -> String {
    let mut out = String::new();
    for segment in template.segments.iter() {
        match segment {
            TemplateSegment::Literal(text) => {
                for c in text.chars() {
                    if c == '{' || c == '}' {
                        out.push(c);
                    }
                    out.push(c);
                }
            }
            TemplateSegment::Reference { name, path } => {
                out.push('{');
                out.push_str(name);
                if !path.is_empty() {
                    out.push('#');
                    for (i, segment) in path.iter().enumerate() {
                        match segment {
                            PathSegment::Key(key) => {
                                if i > 0 {
                                    out.push('.');
                                }
                                out.push_str(key);
                            }
                            PathSegment::Index(index) => {
                                out.push('[');
                                out.push_str(&index.to_string());
                                out.push(']');
                            }
                        }
                    }
                }
                out.push('}');
            }
        }
    }
    out
}
