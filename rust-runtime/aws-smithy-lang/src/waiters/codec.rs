/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Decoding and encoding `smithy.waiters#waitable` trait values.

use super::{
    Acceptor, AcceptorState, LocatedString, Matcher, MatcherKind, PathComparator, PathMatcher,
    Waitable, Waiter,
};
use crate::diagnostic::Location;
use crate::node::{Node, NodeObject};
use crate::traits::TraitDecodeError;
use indexmap::IndexMap;
use std::fmt;

type Result<T> = std::result::Result<T, TraitDecodeError>;

const DOCUMENTATION: &str = "documentation";
const ACCEPTORS: &str = "acceptors";
const MIN_DELAY: &str = "minDelay";
const MAX_DELAY: &str = "maxDelay";
const DEPRECATED: &str = "deprecated";
const TAGS: &str = "tags";
const STATE: &str = "state";
const MATCHER: &str = "matcher";
const MESSAGE: &str = "message";
const OUTPUT: &str = "output";
const INPUT_OUTPUT: &str = "inputOutput";
const SUCCESS: &str = "success";
const ERROR_TYPE: &str = "errorType";
const PATH: &str = "path";
const EXPECTED: &str = "expected";
const COMPARATOR: &str = "comparator";

const DEFAULT_MIN_DELAY: u32 = 2;
const DEFAULT_MAX_DELAY: u32 = 120;
/// The trait models delays as an `integer` of at least 1.
const DELAY_RANGE: std::ops::RangeInclusive<u32> = 1..=i32::MAX as u32;

const WAITER_NAME_PATTERN: &str = "^[A-Z]+[A-Za-z0-9]*$";

/// Where a problem is, as the prefix of an error message: a waiter and, within it, an
/// acceptor. The error's location identifies the exact value; this names it for values
/// created in code, which have no location.
#[derive(Clone, Copy)]
struct Context<'a> {
    waiter: &'a str,
    acceptor: Option<usize>,
}

impl Context<'_> {
    fn error(self, node: &Node, message: impl fmt::Display) -> TraitDecodeError {
        TraitDecodeError::at(node, format!("{self}: {message}"))
    }

    fn type_error(self, node: &Node, name: Option<&str>, expected: &str) -> TraitDecodeError {
        let actual = node.type_name();
        match name {
            Some(name) => self.error(
                node,
                format_args!("expected `{name}` to be {expected}, found {actual}"),
            ),
            None => self.error(node, format_args!("expected {expected}, found {actual}")),
        }
    }
}

impl fmt::Display for Context<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "waiter `{}`", self.waiter)?;
        match self.acceptor {
            Some(index) => write!(f, " acceptor {index}"),
            None => Ok(()),
        }
    }
}

pub(super) fn decode(value: &Node) -> Result<Waitable> {
    let object = value.as_object().ok_or_else(|| {
        TraitDecodeError::at(
            value,
            format!(
                "expected an object of named waiters, found {}",
                value.type_name()
            ),
        )
    })?;
    if object.is_empty() {
        return Err(TraitDecodeError::at(value, "expected at least one waiter"));
    }
    let waiters = object
        .iter()
        .map(|(name, waiter)| {
            if !is_waiter_name(name) {
                return Err(TraitDecodeError::at(
                    waiter,
                    format!("waiter name `{name}` does not match `{WAITER_NAME_PATTERN}`"),
                ));
            }
            let context = Context {
                waiter: name,
                acceptor: None,
            };
            Ok((name.to_owned(), decode_waiter(waiter, context)?))
        })
        .collect::<Result<IndexMap<_, _>>>()?;
    Ok(Waitable {
        waiters,
        location: Location(value.source_location().clone()),
    })
}

/// Whether `name` matches [`WAITER_NAME_PATTERN`].
fn is_waiter_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_uppercase()) && chars.all(|c| c.is_ascii_alphanumeric())
}

fn decode_waiter(value: &Node, context: Context<'_>) -> Result<Waiter> {
    let object = expect_object(value, None, context)?;
    let documentation = optional(object, DOCUMENTATION, context, string)?.map(str::to_owned);

    let acceptors = required(value, object, ACCEPTORS, context)?;
    let elements = acceptors
        .as_array()
        .ok_or_else(|| context.type_error(acceptors, Some(ACCEPTORS), "an array"))?;
    if elements.is_empty() {
        return Err(context.error(acceptors, "expected at least one acceptor"));
    }
    let acceptors = elements
        .iter()
        .enumerate()
        .map(|(index, acceptor)| {
            let context = Context {
                acceptor: Some(index),
                ..context
            };
            decode_acceptor(acceptor, context)
        })
        .collect::<Result<_>>()?;

    let min_delay = optional(object, MIN_DELAY, context, delay)?.unwrap_or(DEFAULT_MIN_DELAY);
    let max_delay = optional(object, MAX_DELAY, context, delay)?.unwrap_or(DEFAULT_MAX_DELAY);
    let deprecated = optional(object, DEPRECATED, context, boolean)?.unwrap_or(false);
    let tags = optional(object, TAGS, context, decode_tags)?.unwrap_or_default();
    Ok(Waiter {
        documentation,
        acceptors,
        min_delay,
        max_delay,
        deprecated,
        tags,
        location: Location(value.source_location().clone()),
    })
}

fn decode_acceptor(value: &Node, context: Context<'_>) -> Result<Acceptor> {
    let object = expect_object(value, None, context)?;
    let state = required(value, object, STATE, context)?;
    let state = match string(state, STATE, context)? {
        "success" => AcceptorState::Success,
        "failure" => AcceptorState::Failure,
        "retry" => AcceptorState::Retry,
        unknown => {
            return Err(context.error(
                state,
                format_args!(
                    "unknown acceptor state `{unknown}`; expected `success`, `failure`, or `retry`"
                ),
            ))
        }
    };
    let matcher = decode_matcher(required(value, object, MATCHER, context)?, context)?;
    let message = optional(object, MESSAGE, context, located_string)?;
    Ok(Acceptor {
        state,
        matcher,
        message,
        location: Location(value.source_location().clone()),
    })
}

fn decode_matcher(value: &Node, context: Context<'_>) -> Result<Matcher> {
    let object = expect_object(value, Some(MATCHER), context)?;
    let mut members = object.iter();
    let (Some((name, member)), None) = (members.next(), members.next()) else {
        return Err(context.error(
            value,
            format_args!(
                "expected `matcher` to set exactly one member, found {}",
                object.len()
            ),
        ));
    };
    let kind = match name {
        OUTPUT => MatcherKind::Output(decode_path_matcher(member, OUTPUT, context)?),
        INPUT_OUTPUT => {
            MatcherKind::InputOutput(decode_path_matcher(member, INPUT_OUTPUT, context)?)
        }
        SUCCESS => MatcherKind::Success(boolean(member, SUCCESS, context)?),
        ERROR_TYPE => MatcherKind::ErrorType(string(member, ERROR_TYPE, context)?.to_owned()),
        unknown => {
            return Err(context.error(
                member,
                format_args!(
                    "unknown matcher member `{unknown}`; expected `{OUTPUT}`, `{INPUT_OUTPUT}`, \
                     `{SUCCESS}`, or `{ERROR_TYPE}`"
                ),
            ))
        }
    };
    Ok(Matcher {
        kind,
        location: Location(member.source_location().clone()),
    })
}

fn decode_path_matcher(value: &Node, name: &str, context: Context<'_>) -> Result<PathMatcher> {
    let object = expect_object(value, Some(name), context)?;
    let path = located_string(required(value, object, PATH, context)?, PATH, context)?;
    let expected = string(
        required(value, object, EXPECTED, context)?,
        EXPECTED,
        context,
    )?;
    let comparator = required(value, object, COMPARATOR, context)?;
    let comparator = match string(comparator, COMPARATOR, context)? {
        "stringEquals" => PathComparator::StringEquals,
        "booleanEquals" => PathComparator::BooleanEquals,
        "allStringEquals" => PathComparator::AllStringEquals,
        "anyStringEquals" => PathComparator::AnyStringEquals,
        unknown => {
            return Err(context.error(
                comparator,
                format_args!(
                    "unknown path comparator `{unknown}`; expected `stringEquals`, \
                     `booleanEquals`, `allStringEquals`, or `anyStringEquals`"
                ),
            ))
        }
    };
    Ok(PathMatcher {
        path,
        expected: expected.to_owned(),
        comparator,
        location: Location(value.source_location().clone()),
    })
}

fn expect_object<'a>(
    value: &'a Node,
    name: Option<&str>,
    context: Context<'_>,
) -> Result<&'a NodeObject> {
    value
        .as_object()
        .ok_or_else(|| context.type_error(value, name, "an object"))
}

fn required<'a>(
    owner: &Node,
    object: &'a NodeObject,
    name: &str,
    context: Context<'_>,
) -> Result<&'a Node> {
    object
        .get(name)
        .ok_or_else(|| context.error(owner, format_args!("missing required member `{name}`")))
}

/// Decodes property `name` of `object` with `decode`, if it is present.
fn optional<'a, T>(
    object: &'a NodeObject,
    name: &str,
    context: Context<'_>,
    decode: impl FnOnce(&'a Node, &str, Context<'_>) -> Result<T>,
) -> Result<Option<T>> {
    object
        .get(name)
        .map(|value| decode(value, name, context))
        .transpose()
}

fn string<'a>(value: &'a Node, name: &str, context: Context<'_>) -> Result<&'a str> {
    value
        .as_str()
        .ok_or_else(|| context.type_error(value, Some(name), "a string"))
}

fn located_string(value: &Node, name: &str, context: Context<'_>) -> Result<LocatedString> {
    Ok(LocatedString {
        value: string(value, name, context)?.to_owned(),
        location: Location(value.source_location().clone()),
    })
}

fn boolean(value: &Node, name: &str, context: Context<'_>) -> Result<bool> {
    value
        .as_bool()
        .ok_or_else(|| context.type_error(value, Some(name), "a boolean"))
}

/// A delay in seconds. Unlike the reference implementation's typed value, which truncates,
/// fractional and out-of-range values are rejected, as the reference implementation's
/// validation of the trait value against its `integer` shape does.
fn delay(value: &Node, name: &str, context: Context<'_>) -> Result<u32> {
    let number = value
        .as_number()
        .ok_or_else(|| context.type_error(value, Some(name), "an integer"))?;
    if !number.is_integer() {
        return Err(context.error(
            value,
            format_args!("expected `{name}` to be an integer, found {number}"),
        ));
    }
    number
        .as_u64()
        .and_then(|delay| u32::try_from(delay).ok())
        .filter(|delay| DELAY_RANGE.contains(delay))
        .ok_or_else(|| {
            context.error(
                value,
                format_args!(
                    "expected `{name}` to be from {} to {}, found {number}",
                    DELAY_RANGE.start(),
                    DELAY_RANGE.end()
                ),
            )
        })
}

fn decode_tags(value: &Node, name: &str, context: Context<'_>) -> Result<Box<[String]>> {
    value
        .as_array()
        .ok_or_else(|| context.type_error(value, Some(name), "an array"))?
        .iter()
        .map(|tag| match tag.as_str() {
            Some("") => Err(context.error(tag, "expected a non-empty tag")),
            Some(tag) => Ok(tag.to_owned()),
            None => Err(context.type_error(tag, None, "a tag string")),
        })
        .collect()
}

pub(super) fn encode(waitable: &Waitable) -> Node {
    Node::from(
        waitable
            .waiters
            .iter()
            .map(|(name, waiter)| (name.clone(), encode_waiter(waiter)))
            .collect::<NodeObject>(),
    )
}

/// Encodes in the reference implementation's order, omitting default values as it does.
fn encode_waiter(waiter: &Waiter) -> Node {
    let mut object = NodeObject::new();
    if let Some(documentation) = &waiter.documentation {
        object.insert(DOCUMENTATION, documentation.clone());
    }
    let acceptors = waiter
        .acceptors
        .iter()
        .map(encode_acceptor)
        .collect::<Vec<_>>();
    object.insert(ACCEPTORS, acceptors);
    if waiter.min_delay != DEFAULT_MIN_DELAY {
        object.insert(MIN_DELAY, u64::from(waiter.min_delay));
    }
    if waiter.max_delay != DEFAULT_MAX_DELAY {
        object.insert(MAX_DELAY, u64::from(waiter.max_delay));
    }
    if waiter.deprecated {
        object.insert(DEPRECATED, true);
    }
    if !waiter.tags.is_empty() {
        let tags = waiter
            .tags
            .iter()
            .cloned()
            .map(Node::from)
            .collect::<Vec<_>>();
        object.insert(TAGS, tags);
    }
    Node::from(object)
}

fn encode_acceptor(acceptor: &Acceptor) -> Node {
    let mut object = NodeObject::new();
    object.insert(STATE, acceptor.state.as_str());
    object.insert(MATCHER, encode_matcher(&acceptor.matcher));
    if let Some(message) = &acceptor.message {
        object.insert(MESSAGE, message.value.clone());
    }
    Node::from(object)
}

fn encode_matcher(matcher: &Matcher) -> Node {
    let (name, value) = match &matcher.kind {
        MatcherKind::Output(path) => (OUTPUT, encode_path_matcher(path)),
        MatcherKind::InputOutput(path) => (INPUT_OUTPUT, encode_path_matcher(path)),
        MatcherKind::Success(success) => (SUCCESS, Node::from(*success)),
        MatcherKind::ErrorType(error) => (ERROR_TYPE, Node::from(error.clone())),
    };
    Node::from(NodeObject::from_iter([(name, value)]))
}

fn encode_path_matcher(matcher: &PathMatcher) -> Node {
    Node::from(NodeObject::from_iter([
        (PATH, Node::from(matcher.path.value.clone())),
        (EXPECTED, Node::from(matcher.expected.clone())),
        (COMPARATOR, Node::from(matcher.comparator.as_str())),
    ]))
}

#[cfg(test)]
mod tests {
    use super::is_waiter_name;

    #[test]
    fn waiter_names_match_the_trait_pattern() {
        for valid in ["A", "AB", "TableExists", "A1", "Ab9Z"] {
            assert!(is_waiter_name(valid), "{valid}");
        }
        for invalid in [
            "",
            "a",
            "tableExists",
            "1A",
            "Table_Exists",
            "Table-Exists",
            "Ä",
        ] {
            assert!(!is_waiter_name(invalid), "{invalid}");
        }
    }
}
