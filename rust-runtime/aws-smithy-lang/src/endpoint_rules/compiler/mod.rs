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

mod builder;
mod cfg;
mod cost;
mod cost_optimization;
mod finish;
mod functions;
mod order;
mod s3;
mod search;
mod sift;
mod ssa;
mod syntax;

#[cfg(test)]
mod tests;

use crate::endpoint_rules::bdd::Bdd;
use crate::endpoint_rules::language::{Expression, ExpressionKind, Rule, RuleKind, RulesVersion};
use crate::endpoint_rules::{EndpointBdd, EndpointRuleSet};
use crate::shape_id::ShapeId;
use builder::Diagram;
use cfg::Cfg;
use std::fmt;

impl EndpointRuleSet {
    /// Compiles the rule set into an [`EndpointBdd`] that selects the same result for all
    /// parameter values. This follows Smithy Java's `CompileBdd` (`smithy-rules-engine` 1.73.0)
    /// and produces the same BDD.
    ///
    /// The rule set must use only standard-library and AWS functions, and every reference
    /// must be in scope (see [`validate`](Self::validate)). The compiler rewrites syntax and
    /// never evaluates a function.
    ///
    /// Compilation is bounded by `options`. Every BDD built, including each variable order the
    /// optimizer tries, must stay within [`CompileOptions::max_nodes`], and the optimizer tries
    /// a number of orders proportional to the number of conditions, which is at most
    /// [`CompileOptions::max_conditions`]. A rule set within the default limits can still take
    /// a long time, so lower them for untrusted input. S3 takes about 2 seconds in an
    /// optimized build.
    pub fn compile(&self, options: &CompileOptions) -> Result<EndpointBdd, CompileError> {
        compile(self, options)
    }
}

/// Options for [`EndpointRuleSet::compile`].
///
/// The default limits are far above every AWS service model at the time of writing: at most
/// 82 conditions after the compiler's rewrites, and 14,256 nodes in any BDD built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileOptions {
    service: Option<ShapeId>,
    max_conditions: usize,
    max_nodes: usize,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            service: None,
            max_conditions: 1_024,
            max_nodes: 1_000_000,
        }
    }
}

impl CompileOptions {
    /// The service whose rules are compiled, if set.
    pub fn service(&self) -> Option<&ShapeId> {
        self.service.as_ref()
    }

    /// Sets the service whose rules are compiled. For `com.amazonaws.s3#AmazonS3`, the rule
    /// set is first rewritten as Smithy Java's `CompileBddForAws` does: availability zone IDs
    /// and S3 Express endpoints are put in a shared form, which shrinks the BDD.
    ///
    /// Java also checks the rewritten rules against S3's endpoint tests, because the rewrite
    /// is only correct given the structure of S3's rules. That needs evaluation, so it is not
    /// done here; a caller compiling a changed S3 rule set should run those tests itself.
    pub fn with_service(mut self, service: ShapeId) -> Self {
        self.service = Some(service);
        self
    }

    /// The most distinct conditions the BDD may test, after the compiler's rewrites.
    pub fn max_conditions(&self) -> usize {
        self.max_conditions
    }

    /// Sets [`max_conditions`](Self::max_conditions).
    pub fn with_max_conditions(mut self, value: usize) -> Self {
        self.max_conditions = value;
        self
    }

    /// The most nodes, including intermediate ones, that building one BDD may create.
    pub fn max_nodes(&self) -> usize {
        self.max_nodes
    }

    /// Sets [`max_nodes`](Self::max_nodes).
    pub fn with_max_nodes(mut self, value: usize) -> Self {
        self.max_nodes = value;
        self
    }
}

/// Why a rule set could not be compiled.
#[derive(Debug)]
pub struct CompileError {
    message: String,
    resource_limit: bool,
}

impl CompileError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            resource_limit: false,
        }
    }

    fn limit(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            resource_limit: true,
        }
    }

    /// Whether compilation stopped at a [`CompileOptions`] limit, rather than because the
    /// rule set cannot be compiled.
    pub fn is_resource_limit(&self) -> bool {
        self.resource_limit
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
    options: &CompileOptions,
) -> Result<(EndpointRuleSet, Cfg, Diagram), CompileError> {
    check(rule_set)?;
    let ssa = ssa::transform(rule_set)?;
    let cfg = Cfg::new(&ssa);
    if cfg.canonical.len() > options.max_conditions {
        return Err(CompileError::limit(format!(
            "the BDD would test {} conditions, more than the limit of {}",
            cfg.canonical.len(),
            options.max_conditions
        )));
    }
    let order = order::initial_order(&cfg, &cfg.condition_order());
    let diagram = builder::compile(&cfg, order, options.max_nodes)?;
    Ok((ssa, cfg, diagram))
}

/// Compiles `rule_set` as Java's `CompileBdd` does: builds the BDD, sifts it, lowers its
/// expected cost, reverses its nodes, and drops conditions no node tests. S3's rule set is
/// first rewritten (see [`s3`]).
fn compile(
    rule_set: &EndpointRuleSet,
    options: &CompileOptions,
) -> Result<EndpointBdd, CompileError> {
    let rewritten;
    let rule_set = match &options.service {
        Some(service) if service.as_str() == s3::SERVICE => {
            rewritten = s3::rewrite(rule_set);
            &rewritten
        }
        _ => rule_set,
    };
    let (ssa, cfg, diagram) = unoptimized(rule_set, options)?;
    let diagram = sift::sift(&cfg, diagram, options.max_nodes)?;
    let diagram = cost_optimization::optimize(&cfg, diagram, options.max_nodes)?;
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
