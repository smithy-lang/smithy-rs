/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Control Flow Graph based on Smithy Java's `Cfg` and `CfgBuilder` representing the SSA
//! rule set as a decision graph.
//!
//! Each rule becomes a chain of condition nodes ending in its result, and falls through to the
//! next rule when a condition fails. Conditions are canonicalized, results are stripped of
//! their conditions, and both, like the nodes, are hash-consed.

use super::syntax::{as_call, call, canonicalize, key, literal, with_function};
use crate::endpoint_rules::language::{
    Condition, ExpressionKind, Literal, ResultKind, ResultRule, Rule, RuleKind,
};
use crate::endpoint_rules::EndpointRuleSet;
use std::collections::{HashMap, HashSet};

/// A node index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct NodeId(usize);

/// A canonical condition, possibly negated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct ConditionRef {
    /// Indexes [`Cfg::canonical`].
    pub(super) index: usize,
    pub(super) negated: bool,
}

#[derive(Clone, Debug)]
pub(super) enum Node {
    Condition {
        condition: ConditionRef,
        /// Taken when the condition (after negation) holds.
        high: NodeId,
        low: NodeId,
    },
    /// An index into [`Cfg::results`], or `None` for no match.
    Result(Option<usize>),
}

/// The decision graph.
#[derive(Debug)]
pub(super) struct Cfg {
    pub(super) nodes: Vec<Node>,
    pub(super) root: NodeId,
    /// Canonical conditions, in the order they were created.
    pub(super) canonical: Vec<Condition>,
    /// Endpoint and error results, in the order they were created.
    pub(super) results: Vec<ResultRule>,
}

impl Cfg {
    /// Builds the graph of an SSA rule set.
    pub(super) fn new(rule_set: &EndpointRuleSet) -> Self {
        let mut builder = Builder::default();
        let terminal = builder.node(Node::Result(None));
        let root = builder.chain(&rule_set.rules, terminal);
        Cfg {
            nodes: builder.nodes,
            root,
            canonical: builder.canonical,
            results: builder.results,
        }
    }

    pub(super) fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0]
    }

    /// The indexes of the canonical conditions in the order a depth-first walk from the root,
    /// high branches first, first reaches them: Java's `Cfg.getConditions()`.
    pub(super) fn condition_order(&self) -> Vec<usize> {
        let mut order = Vec::new();
        let mut seen = HashSet::new();
        let mut visited = HashSet::new();
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            if !visited.insert(id) {
                continue;
            }
            if let Node::Condition {
                condition,
                high,
                low,
            } = self.node(id)
            {
                stack.extend([*low, *high]);
                if seen.insert(condition.index) {
                    order.push(condition.index);
                }
            }
        }
        order
    }
}

#[derive(Default)]
struct Builder {
    nodes: Vec<Node>,
    node_ids: HashMap<(ConditionRef, NodeId, NodeId), NodeId>,
    canonical: Vec<Condition>,
    canonical_ids: HashMap<Condition, usize>,
    /// The reference for each condition seen, as written or canonical.
    references: HashMap<Condition, ConditionRef>,
    /// The first canonical assignment of each function, by its Java text.
    function_bindings: HashMap<String, Condition>,
    results: Vec<ResultRule>,
    result_ids: HashMap<ResultRule, NodeId>,
}

impl Builder {
    fn node(&mut self, node: Node) -> NodeId {
        self.nodes.push(node);
        NodeId(self.nodes.len() - 1)
    }

    /// Rules tried in order, falling through to `fallthrough`. Like Java, rules are converted
    /// last to first, and each rule's conditions last to first.
    fn chain(&mut self, rules: &[Rule], fallthrough: NodeId) -> NodeId {
        rules
            .iter()
            .rev()
            .fold(fallthrough, |next, rule| self.rule(rule, next))
    }

    fn rule(&mut self, rule: &Rule, fallthrough: NodeId) -> NodeId {
        let body = match &rule.kind {
            RuleKind::Tree(children) => self.chain(children, fallthrough),
            RuleKind::Endpoint(endpoint) => {
                self.result(rule, ResultKind::Endpoint(endpoint.clone()))
            }
            RuleKind::Error(error) => self.result(rule, ResultKind::Error(error.clone())),
        };
        rule.conditions.iter().rev().fold(body, |high, condition| {
            let condition = self.reference(condition);
            *self
                .node_ids
                .entry((condition, high, fallthrough))
                .or_insert_with(|| {
                    self.nodes.push(Node::Condition {
                        condition,
                        high,
                        low: fallthrough,
                    });
                    NodeId(self.nodes.len() - 1)
                })
        })
    }

    fn result(&mut self, rule: &Rule, kind: ResultKind) -> NodeId {
        // Java's `Rule.withConditions` drops the documentation when it removes conditions.
        let documentation = if rule.conditions.is_empty() {
            rule.documentation.clone()
        } else {
            None
        };
        let result = ResultRule {
            documentation,
            kind,
            location: rule.location.clone(),
        };
        if let Some(id) = self.result_ids.get(&result) {
            return *id;
        }
        self.results.push(result.clone());
        let id = self.node(Node::Result(Some(self.results.len() - 1)));
        self.result_ids.insert(result, id);
        id
    }

    /// Java's `CfgBuilder.createConditionReference`.
    fn reference(&mut self, condition: &Condition) -> ConditionRef {
        if let Some(reference) = self.references.get(condition) {
            return *reference;
        }
        let mut negated = false;
        let mut canonical = condition.clone();
        // `not(f(...))` is `f(...)`, negated.
        if let (None, Some(("not", [inner]))) = (&condition.assign, as_call(&condition.function)) {
            if let ExpressionKind::Function { .. } = inner.kind {
                negated = true;
                canonical = with_function(condition, inner.clone(), None);
                if let Some(existing) = self.references.get(&canonical) {
                    let reference = ConditionRef {
                        negated: !existing.negated,
                        ..*existing
                    };
                    self.references.insert(condition.clone(), reference);
                    return reference;
                }
            }
        }
        canonical.function = canonicalize(&canonical.function);
        // `booleanEquals(x, false)` is `booleanEquals(x, true)`, negated.
        if let Some(("booleanEquals", [value, flag])) = as_call(&canonical.function) {
            if let ExpressionKind::Literal(Literal::Boolean(false)) = flag.kind {
                let location = &canonical.function.location;
                let truth = literal(Literal::Boolean(true), &flag.location);
                canonical.function = call("booleanEquals", vec![value.clone(), truth], location);
                negated = !negated;
            }
        }
        // `isSet(f(...))` is the condition that assigns `f(...)`, if there is one.
        if let (None, Some(("isSet", [inner]))) = (&canonical.assign, as_call(&canonical.function))
        {
            if let ExpressionKind::Function { .. } = inner.kind {
                if let Some(binding) = self.function_bindings.get(&key(inner)) {
                    canonical = binding.clone();
                }
            }
        }
        if canonical.assign.is_some() {
            self.function_bindings
                .entry(key(&canonical.function))
                .or_insert_with(|| canonical.clone());
        }
        let index = match self.canonical_ids.get(&canonical) {
            Some(index) => *index,
            None => {
                self.canonical.push(canonical.clone());
                self.canonical_ids
                    .insert(canonical.clone(), self.canonical.len() - 1);
                self.canonical.len() - 1
            }
        };
        let reference = ConditionRef { index, negated };
        self.references.insert(condition.clone(), reference);
        if !negated && *condition != canonical {
            self.references.insert(canonical, reference);
        }
        reference
    }
}
