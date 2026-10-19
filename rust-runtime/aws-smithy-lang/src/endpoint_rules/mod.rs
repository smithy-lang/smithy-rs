/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Endpoint rules: decode, validate, inspect, and encode `smithy.rules#endpointBdd` and
//! `smithy.rules#endpointRuleSet`.
//!
//! [`EndpointBdd`] is a [`TraitCodec`] for the endpoint BDD trait. Decoding checks the
//! trait's structure and its binary decision diagram, so a decoded value can be traversed
//! without bounds checks or cycle detection, and its conditions and results are
//! source-located syntax rather than generic [`Node`]s.
//!
//! [`EndpointRuleSet`] is a [`TraitCodec`] for the rule-tree form of the same rules. Both
//! share one language: [`Parameters`], [`Condition`]s, [`Expression`]s, and
//! [`EndpointDefinition`]s. Rule trees are bounded by
//! [`EndpointRulesLimits::max_rule_depth`], and [`EndpointRuleSet::iter_rules`] walks them
//! without recursion.
//!
//! This module represents endpoint rules; it does **not** resolve endpoints. It has no
//! function implementations, parameter values, or evaluator. A client that resolves
//! endpoints supplies those, and follows the traversal contract documented on [`Bdd`].
//!
//! ```
//! use aws_smithy_lang::endpoint_rules::{BddRef, EndpointBdd, ResultKind};
//! use aws_smithy_lang::{Model, ShapeId};
//!
//! let json = r#"{
//!     "smithy": "2.0",
//!     "shapes": {
//!         "ex#Service": {
//!             "type": "service",
//!             "traits": {
//!                 "smithy.rules#endpointBdd": {
//!                     "version": "1.1",
//!                     "parameters": {
//!                         "Region": {"type": "string", "required": false, "builtIn": "AWS::Region"}
//!                     },
//!                     "conditions": [{"fn": "isSet", "argv": [{"ref": "Region"}]}],
//!                     "results": [{
//!                         "type": "endpoint",
//!                         "conditions": [],
//!                         "endpoint": {"url": "https://{Region}.example.com"}
//!                     }],
//!                     "root": 2,
//!                     "nodeCount": 2,
//!                     "nodes": "/////wAAAAH/////AAAAAAX14QEF9eEA"
//!                 }
//!             }
//!         }
//!     }
//! }"#;
//! let model = Model::from_json_str("model.json", json)?;
//! let service = model.expect_shape(&ShapeId::from_static("ex#Service"))?;
//! let rules = service.get_trait_as::<EndpointBdd>()?.expect("the service has endpoint rules");
//!
//! assert_eq!(rules.parameters().get("Region").unwrap().built_in(), Some("AWS::Region"));
//! // The root tests `isSet(Region)`. A true result leads to the endpoint; a false result
//! // leads to the implicit no-match result.
//! let BddRef::Node(root) = rules.bdd().root() else { unreachable!() };
//! let node = rules.bdd().node(root);
//! assert_eq!(rules.conditions()[node.condition()].function_name(), "isSet");
//! let BddRef::Result(matched) = node.high() else { unreachable!() };
//! assert!(matches!(rules.result(matched).unwrap().kind(), ResultKind::Endpoint(_)));
//! let BddRef::Result(unmatched) = node.low() else { unreachable!() };
//! assert!(rules.result(unmatched).is_none());
//! # Ok::<_, Box<dyn std::error::Error>>(())
//! ```
//!
//! # Validation
//!
//! Decoding checks structure. [`EndpointBdd::validate`] and [`EndpointRuleSet::validate`]
//! check name scoping and URL schemes, and [`validator`] checks a model's endpoint rules against the traits that bind their
//! parameters ([`ContextParam`], [`StaticContextParams`], [`OperationContextParams`], and
//! [`ClientContextParams`]).
//!
//! # Consumer contract
//!
//! A client that resolves endpoints owns everything request-time:
//!
//! 1. **Functions:** It maps each condition's [`function_name`](Condition::function_name) to a
//!    trusted implementation, and rejects rules that use a function it does not implement.
//!    This crate preserves unknown names rather than rejecting them.
//! 2. **Parameter values:** It supplies a value for each parameter, applying the modeled
//!    [`default`](Parameter::default) and rejecting a missing [`required`](Parameter::required)
//!    value. The generated smithy-rs clients apply sources in increasing precedence: modeled
//!    defaults, built-in values, [`ClientContextParams`], [`StaticContextParams`],
//!    [`OperationContextParams`], then [`ContextParam`] members.
//! 3. **Evaluation:** It evaluates each condition only when the traversal reaches it. In the
//!    reference implementation a condition is false when its function returns no value or
//!    `false`, and true otherwise; a true condition binds its [`assign`](Condition::assign)
//!    name to the function's result for later conditions and the selected result. It follows
//!    the traversal contract on [`Bdd`].
//! 4. **Results:** It builds its own endpoint or error from the selected [`ResultRule`],
//!    including auth schemes found in endpoint properties.
//! 5. **Budgets:** It bounds request-time work and output size.
//!
//! A minimal traversal, written in client code, looks like this:
//!
//! ```
//! use aws_smithy_lang::endpoint_rules::{BddRef, Condition, EndpointBdd, ResultRule};
//!
//! /// Returns the selected rule, or `None` when no rule matches. `test` evaluates a condition
//! /// with the client's own function implementations and parameter values.
//! fn select<'a>(
//!     rules: &'a EndpointBdd,
//!     mut test: impl FnMut(&Condition) -> bool,
//! ) -> Option<&'a ResultRule> {
//!     let bdd = rules.bdd();
//!     let mut at = bdd.root();
//!     loop {
//!         match at {
//!             BddRef::Node(reference) => {
//!                 let node = bdd.node(reference);
//!                 let value = test(&rules.conditions()[node.condition()]);
//!                 at = if value ^ reference.is_complemented() { node.high() } else { node.low() };
//!             }
//!             BddRef::Result(result) => return rules.result(result),
//!             BddRef::Terminal(_) => return None,
//!         }
//!     }
//! }
//! # use aws_smithy_lang::traits::TraitCodec;
//! # let rules = EndpointBdd::from_node(&aws_smithy_lang::Node::from(
//! #     [
//! #         ("version", aws_smithy_lang::Node::from("1.1")),
//! #         ("parameters", aws_smithy_lang::NodeObject::new().into()),
//! #         ("conditions", vec![aws_smithy_lang::Node::from(
//! #             [("fn", aws_smithy_lang::Node::from("f")), ("argv", vec![].into())]
//! #                 .into_iter()
//! #                 .collect::<aws_smithy_lang::NodeObject>(),
//! #         )].into()),
//! #         ("results", vec![aws_smithy_lang::Node::from(
//! #             [
//! #                 ("type", aws_smithy_lang::Node::from("error")),
//! #                 ("conditions", vec![].into()),
//! #                 ("error", "no".into()),
//! #             ]
//! #             .into_iter()
//! #             .collect::<aws_smithy_lang::NodeObject>(),
//! #         )].into()),
//! #         ("root", 2i64.into()),
//! #         ("nodeCount", 2i64.into()),
//! #         ("nodes", "/////wAAAAH/////AAAAAAX14QEF9eEA".into()),
//! #     ]
//! #     .into_iter()
//! #     .collect::<aws_smithy_lang::NodeObject>(),
//! # ))?;
//! assert!(select(&rules, |_| true).is_some());
//! assert!(select(&rules, |_| false).is_none());
//! # Ok::<_, Box<dyn std::error::Error>>(())
//! ```
//!
//! This crate deliberately provides no such function.
//!
//! [endpoint BDD trait]: https://smithy.io/2.0/additional-specs/rules-engine/specification.html
//! [`TraitCodec`]: crate::traits::TraitCodec
//! [`Node`]: crate::Node

mod bdd;
mod codec;
mod context;
mod language;
mod validate;

pub use bdd::{Bdd, BddNode, BddNodeRef, BddRef, BddResultRef, InvalidBddError};
pub use context::{
    ClientContextParam, ClientContextParams, ContextParam, OperationContextParam,
    OperationContextParams, StaticContextParam, StaticContextParams, CLIENT_CONTEXT_PARAMS,
    CONTEXT_PARAM, OPERATION_CONTEXT_PARAMS, STATIC_CONTEXT_PARAMS,
};
pub use language::{
    Condition, Deprecated, EndpointDefinition, Expression, ExpressionKind, Literal, Parameter,
    ParameterType, ParameterValue, Parameters, PathSegment, ResultKind, ResultRule, Rule, RuleKind,
    RulesVersion, Template, TemplateSegment,
};
pub use validate::validator;

use crate::node::Node;
use crate::shape_id::ShapeId;
use crate::traits::{TraitCodec, TraitDecodeError};

/// `smithy.rules#endpointBdd`
pub const ENDPOINT_BDD: ShapeId = ShapeId::from_static("smithy.rules#endpointBdd");

/// A decoded, structurally valid `smithy.rules#endpointBdd` trait.
///
/// A decoded value guarantees:
///
/// - `bdd().condition_count() == conditions().len()`;
/// - `bdd().result_count() == results().len() + 1`, because result 0 is the implicit
///   no-match result, which is not serialized;
/// - every [`BddNode::condition`] indexes [`conditions`](Self::conditions); and
/// - every non-no-match [`BddResultRef::serialized_index`] indexes
///   [`results`](Self::results).
///
/// Unknown function names are preserved; whether a function exists is up to the consumer.
/// Unknown top-level members are tolerated, as in the reference implementation, and are
/// not re-encoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointBdd {
    version: RulesVersion,
    parameters: Parameters,
    conditions: Box<[Condition]>,
    results: Box<[ResultRule]>,
    bdd: Bdd,
}

impl EndpointBdd {
    /// Decodes a trait value under `limits` rather than the default limits.
    pub fn from_node_with_limits(
        value: &Node,
        limits: &EndpointRulesLimits,
    ) -> Result<Self, TraitDecodeError> {
        codec::decode(value, limits)
    }

    /// The rules-engine version.
    pub fn version(&self) -> RulesVersion {
        self.version
    }

    /// The parameter definitions.
    pub fn parameters(&self) -> &Parameters {
        &self.parameters
    }

    /// The conditions that BDD nodes test, by index.
    pub fn conditions(&self) -> &[Condition] {
        &self.conditions
    }

    /// The serialized results, excluding the implicit no-match result. See
    /// [`BddResultRef::serialized_index`].
    pub fn results(&self) -> &[ResultRule] {
        &self.results
    }

    /// The result addressed by `reference`, or `None` for the implicit no-match result.
    pub fn result(&self, reference: BddResultRef) -> Option<&ResultRule> {
        reference
            .serialized_index()
            .map(|index| &self.results[index])
    }

    /// The checked decision diagram.
    pub fn bdd(&self) -> &Bdd {
        &self.bdd
    }
}

impl TraitCodec for EndpointBdd {
    const ID: ShapeId = ENDPOINT_BDD;

    /// Decodes a trait value under [`EndpointRulesLimits::default`].
    fn from_node(value: &Node) -> Result<Self, TraitDecodeError> {
        codec::decode(value, &EndpointRulesLimits::default())
    }

    fn to_node(&self) -> Node {
        codec::encode(self)
    }
}

/// `smithy.rules#endpointRuleSet`
pub const ENDPOINT_RULE_SET: ShapeId = ShapeId::from_static("smithy.rules#endpointRuleSet");

/// A decoded, structurally valid `smithy.rules#endpointRuleSet` trait.
///
/// The rule set shares its parameter, condition, expression, and endpoint language with
/// [`EndpointBdd`]. Rules form a tree: a client tries [`rules`](Self::rules) in order, and a
/// rule applies when all of its [`conditions`](Rule::conditions) hold. A tree rule then tries
/// its nested rules in order. Assignments are visible to later conditions of the same rule
/// and to everything nested within it, not to sibling rules.
///
/// Unknown function names are preserved. Unknown top-level members are tolerated, as in the
/// reference implementation, and are not re-encoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointRuleSet {
    version: RulesVersion,
    parameters: Parameters,
    rules: Box<[Rule]>,
}

impl EndpointRuleSet {
    /// Decodes a trait value under `limits` rather than the default limits.
    pub fn from_node_with_limits(
        value: &Node,
        limits: &EndpointRulesLimits,
    ) -> Result<Self, TraitDecodeError> {
        codec::decode_rule_set(value, limits)
    }

    /// The rules-engine version.
    pub fn version(&self) -> RulesVersion {
        self.version
    }

    /// The parameter definitions.
    pub fn parameters(&self) -> &Parameters {
        &self.parameters
    }

    /// The top-level rules, in the order they are tried.
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Every rule, including nested rules, in document order (a tree rule precedes the rules
    /// nested within it). The walk uses an explicit stack rather than recursion.
    pub fn iter_rules(&self) -> impl Iterator<Item = &Rule> + '_ {
        let mut stack: Vec<&Rule> = self.rules.iter().rev().collect();
        std::iter::from_fn(move || {
            let rule = stack.pop()?;
            if let RuleKind::Tree(children) = &rule.kind {
                stack.extend(children.iter().rev());
            }
            Some(rule)
        })
    }
}

impl TraitCodec for EndpointRuleSet {
    const ID: ShapeId = ENDPOINT_RULE_SET;

    /// Decodes a trait value under [`EndpointRulesLimits::default`].
    fn from_node(value: &Node) -> Result<Self, TraitDecodeError> {
        codec::decode_rule_set(value, &EndpointRulesLimits::default())
    }

    fn to_node(&self) -> Node {
        codec::encode_rule_set(self)
    }
}

/// Bounds applied while decoding untrusted endpoint rules.
///
/// The defaults are far above every AWS service model at the time of writing (at most 17
/// parameters, 75 BDD conditions, 115 results, 553 wire nodes, 389 rules in a rule set, and
/// rule nesting 18 deep).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointRulesLimits {
    max_parameters: usize,
    max_conditions: usize,
    max_results: usize,
    max_nodes: usize,
    max_expression_depth: usize,
    max_expression_nodes: usize,
    max_template_segments: usize,
    max_rules: usize,
    max_rule_depth: usize,
}

impl Default for EndpointRulesLimits {
    fn default() -> Self {
        Self {
            max_parameters: 1_024,
            max_conditions: 65_536,
            max_results: 65_536,
            max_nodes: 1_000_000,
            max_expression_depth: 100,
            max_expression_nodes: 1_000_000,
            max_template_segments: 1_000_000,
            max_rules: 65_536,
            max_rule_depth: 100,
        }
    }
}

macro_rules! limits {
    ($($(#[$doc:meta])* $field:ident, $setter:ident;)*) => {
        impl EndpointRulesLimits {
            $(
                $(#[$doc])*
                pub fn $field(&self) -> usize {
                    self.$field
                }

                $(#[$doc])*
                pub fn $setter(mut self, value: usize) -> Self {
                    self.$field = value;
                    self
                }
            )*
        }
    };
}

limits! {
    /// The maximum number of parameters.
    max_parameters, with_max_parameters;
    /// The maximum number of conditions, counted across all rules of a rule set.
    max_conditions, with_max_conditions;
    /// The maximum number of serialized results.
    max_results, with_max_results;
    /// The maximum wire node count, including the sentinel.
    max_nodes, with_max_nodes;
    /// The maximum nesting depth of expressions and literals.
    max_expression_depth, with_max_expression_depth;
    /// The maximum number of expressions and literals in one trait value.
    max_expression_nodes, with_max_expression_nodes;
    /// The maximum number of template segments in one trait value.
    max_template_segments, with_max_template_segments;
    /// The maximum number of rules in a rule set, including nested rules.
    max_rules, with_max_rules;
    /// The maximum nesting depth of tree rules in a rule set.
    max_rule_depth, with_max_rule_depth;
}
