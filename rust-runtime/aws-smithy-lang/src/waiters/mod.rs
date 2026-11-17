/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Waiters: decode, inspect, and encode the [`smithy.waiters#waitable`] trait.
//!
//! [`Waitable`] is a [`TraitCodec`] for the trait. A decoded value holds an operation's named
//! [`Waiter`]s, each with its ordered [`Acceptor`]s, and applies the trait's defaults: a
//! `minDelay` of 2 seconds, a `maxDelay` of 120 seconds, not deprecated, and no tags.
//!
//! ```
//! use aws_smithy_lang::waiters::{AcceptorState, MatcherKind, PathComparator, Waitable};
//! use aws_smithy_lang::{Model, ShapeId};
//!
//! let json = r#"{
//!     "smithy": "2.0",
//!     "shapes": {
//!         "ex#DescribeTable": {
//!             "type": "operation",
//!             "traits": {
//!                 "smithy.waiters#waitable": {
//!                     "TableExists": {
//!                         "acceptors": [
//!                             {
//!                                 "state": "success",
//!                                 "matcher": {
//!                                     "output": {
//!                                         "path": "Table.TableStatus",
//!                                         "expected": "ACTIVE",
//!                                         "comparator": "stringEquals"
//!                                     }
//!                                 }
//!                             },
//!                             {"state": "retry", "matcher": {"errorType": "ResourceNotFoundException"}}
//!                         ],
//!                         "minDelay": 20
//!                     }
//!                 }
//!             }
//!         }
//!     }
//! }"#;
//! let model = Model::from_json_str("model.json", json)?;
//! let operation = model.expect_shape(&ShapeId::from_static("ex#DescribeTable"))?;
//! let waitable = operation.get_trait_as::<Waitable>()?.expect("the operation has waiters");
//!
//! let waiter = waitable.get("TableExists").expect("the waiter is defined");
//! assert_eq!((waiter.min_delay(), waiter.max_delay()), (20, 120));
//! let [active, missing] = waiter.acceptors() else { unreachable!() };
//! assert_eq!(active.state(), AcceptorState::Success);
//! let MatcherKind::Output(output) = active.matcher().kind() else { unreachable!() };
//! assert_eq!(output.path(), "Table.TableStatus");
//! assert_eq!(output.comparator(), PathComparator::StringEquals);
//! assert_eq!(
//!     missing.matcher().kind(),
//!     &MatcherKind::ErrorType("ResourceNotFoundException".into())
//! );
//! # Ok::<_, Box<dyn std::error::Error>>(())
//! ```
//!
//! # Decoding
//!
//! Decoding checks the form of each value and reports the first problem at the offending
//! nested value. A decoded value guarantees that:
//!
//! - there is at least one waiter, and every waiter name matches `^[A-Z]+[A-Za-z0-9]*$`;
//! - every waiter has at least one acceptor;
//! - both delays are from 1 to `i32::MAX` seconds, because the trait models them as
//!   `integer`s;
//! - every tag is a non-empty string;
//! - every acceptor state, matcher, and path comparator is one this crate knows.
//!
//! Decoding does not check rules that depend on sibling values, the operation, or the
//! service: that `minDelay` is at most `maxDelay`, that a waiter has a success acceptor, that
//! only failure acceptors have a message, that a `booleanEquals` matcher expects `"true"` or
//! `"false"`, that an `errorType` names an error of the operation, that waiter names are
//! unique within a service, or that the operation does not use event streams.
//!
//! JMESPath expressions ([`PathMatcher::path`] and [`Acceptor::message`]) are kept exactly as
//! written, with the location of their string value. Decoding does not parse them.
//!
//! As in the reference implementation, unknown properties of waiters, acceptors, and path
//! matchers are ignored, and are not re-encoded. An unknown matcher member is an error.
//!
//! # Order and equality
//!
//! Waiters keep their declaration order, and acceptors keep the order in which they are
//! tried. Equality ignores source locations and the order of waiters, like [`Node`] equality;
//! the order of acceptors is significant.
//!
//! Waiters are attached directly to operations, with no service defaults or inheritance, so
//! there is no waiter knowledge index. To find the waiters of a service, decode the trait on
//! each of the service's
//! [contained operations](crate::knowledge::TopDownIndex::get_contained_operations).
//!
//! [`smithy.waiters#waitable`]: https://smithy.io/2.0/additional-specs/waiters.html
//! [`TraitCodec`]: crate::traits::TraitCodec

mod codec;

use crate::diagnostic::{Location, SourceLocation};
use crate::node::Node;
use crate::shape_id::ShapeId;
use crate::traits::{TraitCodec, TraitDecodeError};
use indexmap::IndexMap;

/// `smithy.waiters#waitable`
pub const WAITABLE: ShapeId = ShapeId::from_static("smithy.waiters#waitable");

/// A decoded `smithy.waiters#waitable` trait: an operation's named waiters.
///
/// See the [module documentation](self) for what decoding guarantees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waitable {
    waiters: IndexMap<String, Waiter>,
    location: Location,
}

impl Waitable {
    /// The waiter named `name`, compared case-sensitively.
    pub fn get(&self, name: &str) -> Option<&Waiter> {
        self.waiters.get(name)
    }

    /// The waiters and their names, in declaration order.
    pub fn waiters(&self) -> impl ExactSizeIterator<Item = (&str, &Waiter)> + '_ {
        self.waiters
            .iter()
            .map(|(name, waiter)| (name.as_str(), waiter))
    }

    /// The number of waiters. A decoded value has at least one.
    pub fn len(&self) -> usize {
        self.waiters.len()
    }

    /// Returns `true` if there are no waiters, which is never the case for a decoded value.
    pub fn is_empty(&self) -> bool {
        self.waiters.is_empty()
    }

    /// Where the trait value was loaded from.
    pub fn source_location(&self) -> &SourceLocation {
        &self.location.0
    }
}

impl TraitCodec for Waitable {
    const ID: ShapeId = WAITABLE;

    fn from_node(value: &Node) -> Result<Self, TraitDecodeError> {
        codec::decode(value)
    }

    fn to_node(&self) -> Node {
        codec::encode(self)
    }
}

/// One named waiter: how often to retry an operation, and the acceptors that decide when to
/// stop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waiter {
    documentation: Option<String>,
    acceptors: Box<[Acceptor]>,
    min_delay: u32,
    max_delay: u32,
    deprecated: bool,
    tags: Box<[String]>,
    location: Location,
}

impl Waiter {
    /// The documentation, in CommonMark.
    pub fn documentation(&self) -> Option<&str> {
        self.documentation.as_deref()
    }

    /// The acceptors, in the order they are tried: the first that matches decides the
    /// waiter's next state. A decoded waiter has at least one.
    pub fn acceptors(&self) -> &[Acceptor] {
        &self.acceptors
    }

    /// The minimum delay between attempts, in seconds. Defaults to 2.
    pub fn min_delay(&self) -> u32 {
        self.min_delay
    }

    /// The maximum delay between attempts, in seconds. Defaults to 120.
    pub fn max_delay(&self) -> u32 {
        self.max_delay
    }

    /// Whether the waiter is deprecated. Defaults to `false`.
    pub fn is_deprecated(&self) -> bool {
        self.deprecated
    }

    /// The tags that categorize the waiter, in declaration order. Defaults to none.
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// Where the waiter was loaded from.
    pub fn source_location(&self) -> &SourceLocation {
        &self.location.0
    }
}

/// A transition in a waiter's state machine: when the matcher matches, the waiter moves to
/// the acceptor's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Acceptor {
    state: AcceptorState,
    matcher: Matcher,
    message: Option<LocatedString>,
    location: Location,
}

impl Acceptor {
    /// The state the waiter moves to when the matcher matches.
    pub fn state(&self) -> AcceptorState {
        self.state
    }

    /// What the acceptor matches.
    pub fn matcher(&self) -> &Matcher {
        &self.matcher
    }

    /// The JMESPath expression that extracts a human-readable message when the acceptor
    /// matches, exactly as written. It is not parsed.
    pub fn message(&self) -> Option<&str> {
        self.message.as_ref().map(|message| message.value.as_str())
    }

    /// Where the [`message`](Self::message) string was loaded from.
    pub fn message_source_location(&self) -> Option<&SourceLocation> {
        self.message.as_ref().map(|message| &message.location.0)
    }

    /// Where the acceptor was loaded from.
    pub fn source_location(&self) -> &SourceLocation {
        &self.location.0
    }
}

/// The state an [`Acceptor`] moves a waiter to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AcceptorState {
    /// `"success"`: the waiter finished waiting. The waiter stops.
    Success,
    /// `"failure"`: the resource will not reach the desired state. The waiter stops.
    Failure,
    /// `"retry"`: the waiter tries the operation again. This is also the state when no
    /// acceptor matches.
    Retry,
}

impl AcceptorState {
    /// The name of the state in a trait value.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            AcceptorState::Success => "success",
            AcceptorState::Failure => "failure",
            AcceptorState::Retry => "retry",
        }
    }
}

/// How an [`Acceptor`] decides whether it matches the result of an operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Matcher {
    kind: MatcherKind,
    location: Location,
}

impl Matcher {
    /// What the matcher tests.
    pub fn kind(&self) -> &MatcherKind {
        &self.kind
    }

    /// Where the matcher's member value was loaded from: for example the `errorType` string
    /// of an `errorType` matcher.
    pub fn source_location(&self) -> &SourceLocation {
        &self.location.0
    }
}

/// What a [`Matcher`] tests.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MatcherKind {
    /// `output`: tests the output of a successful operation.
    Output(PathMatcher),
    /// `inputOutput`: tests an object whose `input` and `output` members are the input and
    /// output of a successful operation.
    InputOutput(PathMatcher),
    /// `success`: when `true`, matches any successful operation; when `false`, matches any
    /// error.
    Success(bool),
    /// `errorType`: matches an error with this name. As written, this is either an absolute
    /// shape ID, matched exactly, or a shape name, matched in any namespace. It may name an
    /// error the operation does not model.
    ErrorType(String),
}

/// Tests the result of a JMESPath expression against an expected value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathMatcher {
    path: LocatedString,
    expected: String,
    comparator: PathComparator,
    location: Location,
}

impl PathMatcher {
    /// The JMESPath expression, exactly as written. It is not parsed.
    pub fn path(&self) -> &str {
        &self.path.value
    }

    /// The value the comparator compares the result of the expression with.
    pub fn expected(&self) -> &str {
        &self.expected
    }

    /// How the result of the expression is compared with the expected value.
    pub fn comparator(&self) -> PathComparator {
        self.comparator
    }

    /// Where the [`path`](Self::path) string was loaded from.
    pub fn path_source_location(&self) -> &SourceLocation {
        &self.path.location.0
    }

    /// Where the path matcher was loaded from.
    pub fn source_location(&self) -> &SourceLocation {
        &self.location.0
    }
}

/// How a [`PathMatcher`] compares the result of its expression with the expected value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PathComparator {
    /// `"stringEquals"`: the result is a string equal to the expected value.
    StringEquals,
    /// `"booleanEquals"`: the result is a boolean equal to the expected value, `"true"` or
    /// `"false"`.
    BooleanEquals,
    /// `"allStringEquals"`: the result is a non-empty array of strings that all equal the
    /// expected value.
    AllStringEquals,
    /// `"anyStringEquals"`: the result is an array of strings, at least one of which equals
    /// the expected value.
    AnyStringEquals,
}

impl PathComparator {
    /// The name of the comparator in a trait value.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            PathComparator::StringEquals => "stringEquals",
            PathComparator::BooleanEquals => "booleanEquals",
            PathComparator::AllStringEquals => "allStringEquals",
            PathComparator::AnyStringEquals => "anyStringEquals",
        }
    }
}

/// A string and the location of its node, which equality ignores.
#[derive(Clone, Debug, PartialEq, Eq)]
struct LocatedString {
    value: String,
    location: Location,
}
