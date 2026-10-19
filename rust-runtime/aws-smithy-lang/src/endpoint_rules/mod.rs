/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Endpoint rules: decode, validate, inspect, and encode `smithy.rules#endpointBdd`.
//!
//! [`EndpointBdd`] is a [`TraitCodec`] for the [endpoint BDD trait]. Decoding checks the
//! trait's structure and its binary decision diagram, so a decoded value can be traversed
//! without bounds checks or cycle detection, and its conditions and results are
//! source-located syntax rather than generic [`Node`]s.
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
//! [endpoint BDD trait]: https://smithy.io/2.0/additional-specs/rules-engine/specification.html
//! [`TraitCodec`]: crate::traits::TraitCodec
//! [`Node`]: crate::Node

mod bdd;
mod codec;
mod language;

pub use bdd::{Bdd, BddNode, BddNodeRef, BddRef, BddResultRef, InvalidBddError};
pub use language::{
    Condition, Deprecated, EndpointDefinition, Expression, ExpressionKind, Literal, Parameter,
    ParameterType, ParameterValue, Parameters, PathSegment, ResultKind, ResultRule, RulesVersion,
    Template, TemplateSegment,
};

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

/// Bounds applied while decoding untrusted endpoint rules.
///
/// The defaults are far above every AWS service model at the time of writing (at most 17
/// parameters, 75 conditions, 115 results, and 553 wire nodes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointRulesLimits {
    max_parameters: usize,
    max_conditions: usize,
    max_results: usize,
    max_nodes: usize,
    max_expression_depth: usize,
    max_expression_nodes: usize,
    max_template_segments: usize,
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
    /// The maximum number of conditions.
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
}
