/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Compiles an [`EndpointRuleSet`] into an endpoint BDD, following Smithy Java's
//! `CompileBdd` (`smithy-rules-engine` 1.73.0) stage by stage, so it produces the BDDs Java
//! does.
//!
//! Where Java's compiler fails on a valid rule set, or produces a BDD that selects a different
//! result than the rule set, this compiler does not; each difference is marked "Java" where it
//! is made. `tests/fixtures/endpoint_bdd` holds every stage of Java's output, with
//! `java-fixes.patch` applied for the rule sets Java gets wrong.
//!
//! Nothing here evaluates an expression: the compiler only rewrites syntax, using the static
//! facts in [`functions`].

// The compiler is only exercised by its parity tests until it has a public entry point.
#![cfg_attr(not(test), allow(dead_code))]

mod builder;
mod cfg;
mod cost;
mod cost_optimization;
mod finish;
mod functions;
mod order;
mod search;
mod sift;
mod ssa;
mod syntax;

#[cfg(test)]
mod tests;

use crate::endpoint_rules::bdd::Bdd;
use crate::endpoint_rules::language::{Expression, ExpressionKind, Rule, RuleKind, RulesVersion};
use crate::endpoint_rules::{EndpointBdd, EndpointRuleSet};
use builder::Diagram;
use cfg::Cfg;
use std::fmt;

/// Why a rule set could not be compiled.
#[derive(Debug)]
pub(crate) struct CompileError {
    message: String,
}

impl CompileError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CompileError {}

/// Checks what the stages rely on: every function is known, and every name is assigned and
/// referenced in scope.
fn check(rule_set: &EndpointRuleSet) -> Result<(), CompileError> {
    if let Some(error) = rule_set.scope_errors().into_iter().next() {
        return Err(CompileError::new(error.message().to_owned()));
    }
    fn known(expression: &Expression) -> Result<(), CompileError> {
        if let ExpressionKind::Function { name, arguments } = &expression.kind {
            if functions::get(name).is_none() {
                return Err(CompileError::new(format!("unknown function `{name}`")));
            }
            arguments.iter().try_for_each(known)?;
        }
        Ok(())
    }
    let mut stack: Vec<&Rule> = rule_set.rules.iter().collect();
    while let Some(rule) = stack.pop() {
        rule.conditions
            .iter()
            .try_for_each(|c| known(&c.function))?;
        match &rule.kind {
            RuleKind::Tree(children) => stack.extend(children.iter()),
            RuleKind::Endpoint(endpoint) => {
                known(&endpoint.url)?;
                endpoint
                    .headers
                    .iter()
                    .flat_map(|(_, values)| values.iter())
                    .try_for_each(known)?;
            }
            RuleKind::Error(error) => known(error)?,
        }
    }
    Ok(())
}

/// The SSA rule set, its CFG, and the BDD before optimization (Java's `EndpointBddTrait.from`).
fn unoptimized(
    rule_set: &EndpointRuleSet,
) -> Result<(EndpointRuleSet, Cfg, Diagram), CompileError> {
    check(rule_set)?;
    let ssa = ssa::transform(rule_set)?;
    let cfg = Cfg::new(&ssa);
    let order = order::initial_order(&cfg, &cfg.condition_order());
    let diagram = builder::compile(&cfg, order)?;
    Ok((ssa, cfg, diagram))
}

/// Compiles `rule_set` as Java's `CompileBdd` does: builds the BDD, sifts it, lowers its
/// expected cost, reverses its nodes, and drops conditions no node tests.
fn compile(rule_set: &EndpointRuleSet) -> Result<EndpointBdd, CompileError> {
    let (ssa, cfg, diagram) = unoptimized(rule_set)?;
    let diagram = sift::sift(&cfg, diagram)?;
    let diagram = cost_optimization::optimize(&cfg, diagram)?;
    let diagram = finish::reverse(diagram);
    let diagram = finish::remove_unreferenced_conditions(&cfg, diagram);
    endpoint_bdd(&ssa, &cfg, &diagram)
}

/// Publishes a diagram as a checked [`EndpointBdd`].
fn endpoint_bdd(
    rule_set: &EndpointRuleSet,
    cfg: &Cfg,
    diagram: &Diagram,
) -> Result<EndpointBdd, CompileError> {
    let conditions: Box<[_]> = diagram
        .conditions
        .iter()
        .map(|&i| cfg.canonical[i].clone())
        .collect();
    let results: Box<[_]> = diagram
        .results
        .iter()
        .map(|&i| cfg.results[i].clone())
        .collect();
    let bdd = Bdd::from_encoded(
        diagram.root,
        &diagram.nodes,
        conditions.len(),
        results.len() + 1,
    )
    .map_err(|e| CompileError::new(format!("the compiled BDD is invalid: {e}")))?;
    Ok(EndpointBdd {
        // The BDD trait requires version 1.1, which only adds syntax.
        version: rule_set.version.max(RulesVersion::V1_1),
        parameters: rule_set.parameters.clone(),
        conditions,
        results,
        bdd,
    })
}
