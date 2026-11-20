/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Smithy Java's `SsaTransform`: five rule-set rewrites, then a renaming that makes every
//! assignment name unique to the expression it binds.
//!
//! Each rewrite follows the Java class of the same name. Where Java's rewrite can produce an
//! invalid or semantically different rule set, this one does not; each difference is marked
//! "Java" and matches `tests/fixtures/endpoint_bdd/java-fixes.patch`.

use super::functions::{self, ReturnType};
use super::syntax::{
    as_call, as_reference, call, canonicalize, count_references, key, literal, reference,
    reference_set, rename, rename_result, result_values, with_function,
};
use super::CompileError;
use crate::endpoint_rules::language::{
    Condition, Expression, ExpressionKind, Literal, Rule, RuleKind, TemplateSegment,
};
use crate::endpoint_rules::EndpointRuleSet;
use std::collections::{BTreeSet, HashMap, HashSet};

/// Applies the rewrites and the SSA renaming to a valid rule set.
pub(super) fn transform(rule_set: &EndpointRuleSet) -> Result<EndpointRuleSet, CompileError> {
    let with_rules = |rules| EndpointRuleSet {
        version: rule_set.version,
        parameters: rule_set.parameters.clone(),
        rules,
    };
    let rules = coalesce_is_set_booleans(&rule_set.rules);
    let rules = SyntheticBindings::default().rules(&rules);
    let rule_set = consolidate_variables(&with_rules(rules));
    let rules = eliminate_dead_stores(&rule_set.rules);
    let rules = Coalesce::default().rules(&rules);
    let rule_set = with_rules(rules);
    let rules = Ssa::new(&rule_set).rules(&rule_set.rules)?;
    Ok(with_rules(rules))
}

fn map_rules(rules: &[Rule], f: &mut impl FnMut(&Rule) -> Rule) -> Box<[Rule]> {
    rules.iter().map(f).collect()
}

fn with_conditions(rule: &Rule, conditions: Box<[Condition]>, kind: RuleKind) -> Rule {
    Rule {
        conditions,
        documentation: rule.documentation.clone(),
        kind,
        location: rule.location.clone(),
    }
}

/// `IsSetBooleanCoalesceTransform`: `isSet(X)` followed by `booleanEquals(X, b)` becomes
/// `booleanEquals(coalesce(X, !b), b)`.
fn coalesce_is_set_booleans(rules: &[Rule]) -> Box<[Rule]> {
    map_rules(rules, &mut |rule| {
        let mut conditions = Vec::new();
        let mut i = 0;
        while i < rule.conditions.len() {
            let current = &rule.conditions[i];
            if let (None, Some(("isSet", [argument])), Some(next)) = (
                &current.assign,
                as_call(&current.function),
                rule.conditions.get(i + 1),
            ) {
                if let Some(coalesced) =
                    as_reference(argument).and_then(|r| is_set_boolean(r, next))
                {
                    conditions.push(coalesced);
                    i += 2;
                    continue;
                }
            }
            conditions.push(current.clone());
            i += 1;
        }
        let kind = match &rule.kind {
            RuleKind::Tree(children) => RuleKind::Tree(coalesce_is_set_booleans(children)),
            kind => kind.clone(),
        };
        with_conditions(rule, conditions.into(), kind)
    })
}

fn is_set_boolean(name: &str, next: &Condition) -> Option<Condition> {
    let ("booleanEquals", [checked, value]) = as_call(&next.function)? else {
        return None;
    };
    let ExpressionKind::Literal(Literal::Boolean(value)) = value.kind else {
        return None;
    };
    if next.assign.is_some() || as_reference(checked) != Some(name) {
        return None;
    }
    let location = &next.function.location;
    let coalesced = call(
        "coalesce",
        vec![
            reference(name, location),
            literal(Literal::Boolean(!value), location),
        ],
        location,
    );
    let function = call(
        "booleanEquals",
        vec![coalesced, literal(Literal::Boolean(value), location)],
        location,
    );
    Some(with_function(next, function, None))
}

/// `SyntheticBindingTransform`: assigns every unassigned non-boolean condition, and turns
/// `isSet(f(...))` into an assignment of `f(...)`.
#[derive(Default)]
struct SyntheticBindings {
    count: usize,
}

impl SyntheticBindings {
    fn rules(&mut self, rules: &[Rule]) -> Box<[Rule]> {
        map_rules(rules, &mut |rule| {
            let conditions = rule.conditions.iter().map(|c| self.condition(c)).collect();
            let kind = match &rule.kind {
                RuleKind::Tree(children) => RuleKind::Tree(self.rules(children)),
                kind => kind.clone(),
            };
            with_conditions(rule, conditions, kind)
        })
    }

    fn condition(&mut self, condition: &Condition) -> Condition {
        if condition.assign.is_some() {
            return condition.clone();
        }
        let name = condition.function_name();
        if name == "isSet" {
            if let [inner @ Expression {
                kind: ExpressionKind::Function { .. },
                ..
            }] = condition.arguments()
            {
                let assign = self.name(name);
                return with_function(condition, inner.clone(), Some(assign));
            }
            return condition.clone();
        }
        if functions::get(name).is_some_and(|f| f.returns == ReturnType::Boolean) {
            return condition.clone();
        }
        let assign = self.name(name);
        with_function(condition, condition.function.clone(), Some(assign))
    }

    fn name(&mut self, function: &str) -> String {
        let name = format!("_synthetic_{function}_{}", self.count);
        self.count += 1;
        name
    }
}

/// `VariableConsolidationTransform`: an assignment of an expression an ancestor already
/// assigns is removed, and an assignment of an expression assigned elsewhere is renamed to
/// the first name used for it, where that name would not shadow another.
///
/// Java renames by name across the whole rule set, and can then rename into a name already in
/// scope, or rename a removed assignment's references to a different binding. In either case
/// the rule set is returned unchanged.
fn consolidate_variables(rule_set: &EndpointRuleSet) -> EndpointRuleSet {
    let mut consolidation = Consolidation::default();
    for rule in rule_set.rules.iter() {
        consolidation.discover(rule, &HashMap::new(), &HashSet::new());
    }
    let renames = &consolidation.renames;
    if consolidation
        .eliminated_renames
        .iter()
        .any(|(name, parent)| renames.get(name) != Some(parent))
    {
        return rule_set.clone();
    }
    let mut index = 0;
    let rules = consolidation.rewrite(&rule_set.rules, &mut index);
    let consolidated = EndpointRuleSet {
        version: rule_set.version,
        parameters: rule_set.parameters.clone(),
        rules,
    };
    if consolidated.scope_errors().is_empty() {
        consolidated
    } else {
        rule_set.clone()
    }
}

#[derive(Default)]
struct Consolidation {
    /// The first name each canonical expression was assigned to.
    first_names: HashMap<String, String>,
    renames: HashMap<String, String>,
    /// Removed conditions, by document-order index.
    eliminated: HashSet<usize>,
    /// The name of each removed assignment and the ancestor name it was renamed to.
    eliminated_renames: Vec<(String, String)>,
    next_index: usize,
}

impl Consolidation {
    fn discover(
        &mut self,
        rule: &Rule,
        parent_bindings: &HashMap<String, String>,
        ancestor_names: &HashSet<String>,
    ) {
        let mut bindings = parent_bindings.clone();
        let mut visible = ancestor_names.clone();
        for condition in rule.conditions.iter() {
            let index = self.next_index;
            self.next_index += 1;
            let Some(name) = &condition.assign else {
                continue;
            };
            let canonical = key(&canonicalize(&condition.function));
            if let Some(parent) = parent_bindings.get(&canonical) {
                self.renames.insert(name.clone(), parent.clone());
                self.eliminated.insert(index);
                self.eliminated_renames.push((name.clone(), parent.clone()));
                continue;
            }
            bindings.insert(canonical.clone(), name.clone());
            visible.insert(name.clone());
            match self.first_names.get(&canonical) {
                Some(first) if first != name => {
                    if !ancestor_names.contains(first) && !assigned_below(first, rule) {
                        self.renames.insert(name.clone(), first.clone());
                    }
                }
                Some(_) => {}
                None => {
                    self.first_names.insert(canonical, name.clone());
                }
            }
        }
        if let RuleKind::Tree(children) = &rule.kind {
            for child in children.iter() {
                self.discover(child, &bindings, &visible);
            }
        }
    }

    fn rewrite(&self, rules: &[Rule], index: &mut usize) -> Box<[Rule]> {
        let rename_fn = |name: &str| self.renames.get(name).cloned();
        map_rules(rules, &mut |rule| {
            let mut conditions = Vec::new();
            for condition in rule.conditions.iter() {
                *index += 1;
                if self.eliminated.contains(&(*index - 1)) {
                    continue;
                }
                let assign = condition
                    .assign
                    .as_ref()
                    .map(|name| rename_fn(name).unwrap_or_else(|| name.clone()));
                conditions.push(with_function(
                    condition,
                    rename(&condition.function, &rename_fn),
                    assign,
                ));
            }
            let kind = match &rule.kind {
                RuleKind::Tree(children) => RuleKind::Tree(self.rewrite(children, index)),
                kind => rename_result(kind, &rename_fn),
            };
            with_conditions(rule, conditions.into(), kind)
        })
    }
}

/// Whether a rule nested within `rule` assigns `name`.
fn assigned_below(name: &str, rule: &Rule) -> bool {
    let RuleKind::Tree(children) = &rule.kind else {
        return false;
    };
    children.iter().any(|child| {
        child.conditions.iter().any(|c| c.assign() == Some(name)) || assigned_below(name, child)
    })
}

/// Every occurrence of every name the rules reference, in conditions and results.
fn reference_counts(rules: &[Rule], counts: &mut HashMap<String, usize>) {
    for rule in rules {
        for condition in rule.conditions.iter() {
            count_references(&condition.function, counts);
        }
        match &rule.kind {
            RuleKind::Tree(children) => reference_counts(children, counts),
            kind => result_values(kind)
                .iter()
                .for_each(|value| value.count_references(counts)),
        }
    }
}

/// `DeadStoreEliminationTransform`: removes assignments nothing references.
fn eliminate_dead_stores(rules: &[Rule]) -> Box<[Rule]> {
    let mut counts = HashMap::new();
    reference_counts(rules, &mut counts);
    fn strip(rules: &[Rule], counts: &HashMap<String, usize>) -> Box<[Rule]> {
        map_rules(rules, &mut |rule| {
            let conditions = rule
                .conditions
                .iter()
                .map(|condition| match &condition.assign {
                    Some(name) if !counts.contains_key(name) => {
                        with_function(condition, condition.function.clone(), None)
                    }
                    _ => condition.clone(),
                })
                .collect();
            let kind = match &rule.kind {
                RuleKind::Tree(children) => RuleKind::Tree(strip(children, counts)),
                kind => kind.clone(),
            };
            with_conditions(rule, conditions, kind)
        })
    }
    strip(rules, &counts)
}

/// `CoalesceTransform`: an assignment used only by the next condition is inlined into it as
/// `coalesce(expression, zero)`, where `zero` is the zero value of the expression's type.
///
/// Java counts only the uses in the rule's conditions, and inlines even when the next
/// condition holds for the zero value, where the assignment would have failed. This only
/// inlines when the assignment is used nowhere else, and the next condition then fails too.
/// It also never inlines into a template, which Java would rewrite into invalid text.
#[derive(Default)]
struct Coalesce {
    cache: HashMap<[String; 5], Condition>,
}

impl Coalesce {
    fn rules(&mut self, rules: &[Rule]) -> Box<[Rule]> {
        map_rules(rules, &mut |rule| {
            let mut uses = HashMap::new();
            for condition in rule.conditions.iter() {
                for name in reference_set(&condition.function) {
                    *uses.entry(name).or_default() += 1;
                }
            }
            count_result_uses(rule, &mut uses);
            let conditions = self.conditions(&rule.conditions, &uses);
            let kind = match &rule.kind {
                RuleKind::Tree(children) => RuleKind::Tree(self.rules(children)),
                kind => kind.clone(),
            };
            with_conditions(rule, conditions, kind)
        })
    }

    fn conditions(
        &mut self,
        conditions: &[Condition],
        uses: &HashMap<&str, usize>,
    ) -> Box<[Condition]> {
        let mut out = Vec::new();
        // Java skips any later condition equal to an inlined one.
        let mut eliminated: Vec<&Condition> = Vec::new();
        let mut i = 0;
        while i < conditions.len() {
            let current = &conditions[i];
            i += 1;
            if eliminated.contains(&current) {
                continue;
            }
            if let (Some(name), Some(next)) = (&current.assign, conditions.get(i)) {
                if let Some(zero) = can_coalesce(name, current, next, uses) {
                    out.push(self.coalesced(current, next, name, zero));
                    eliminated.extend([current, next]);
                    i += 1;
                    continue;
                }
            }
            out.push(current.clone());
        }
        out.into()
    }

    fn coalesced(
        &mut self,
        bind: &Condition,
        using: &Condition,
        name: &str,
        zero: Literal,
    ) -> Condition {
        let zero = literal(zero, &bind.function.location);
        let cache_key = [
            key(&canonicalize(&bind.function)),
            key(&zero),
            key(&canonicalize(&using.function)),
            name.to_owned(),
            using.assign.clone().unwrap_or_default(),
        ];
        if let Some(cached) = self.cache.get(&cache_key) {
            return cached.clone();
        }
        let location = &bind.function.location;
        let coalesced = call("coalesce", vec![bind.function.clone(), zero], location);
        let function = canonicalize(&replace_reference(&using.function, name, &coalesced));
        let condition = with_function(using, function, using.assign.clone());
        self.cache.insert(cache_key, condition.clone());
        condition
    }
}

/// Counts the names referenced by a rule's result, or by its nested rules' conditions and
/// results: once per condition or result value.
fn count_result_uses<'a>(rule: &'a Rule, uses: &mut HashMap<&'a str, usize>) {
    match &rule.kind {
        RuleKind::Tree(children) => {
            for child in children.iter() {
                for condition in child.conditions.iter() {
                    for name in reference_set(&condition.function) {
                        *uses.entry(name).or_default() += 1;
                    }
                }
                count_result_uses(child, uses);
            }
        }
        kind => {
            for value in result_values(kind) {
                let mut names = BTreeSet::new();
                value.references(&mut names);
                for name in names {
                    *uses.entry(name).or_default() += 1;
                }
            }
        }
    }
}

/// The zero value to coalesce `bind` with, if it can be inlined into `using`.
fn can_coalesce(
    name: &str,
    bind: &Condition,
    using: &Condition,
    uses: &HashMap<&str, usize>,
) -> Option<Literal> {
    let using_name = using.function_name();
    if using_name == "isSet"
        || !reference_set(&using.function).contains(name)
        || uses.get(name) != Some(&1)
        || in_template(&using.function, name)
    {
        return None;
    }
    let returns = functions::get(bind.function_name())?.returns;
    let zero = returns.inner().zero()?;
    // An assignment fails when its value is empty, or false. The inlined condition must then
    // fail too.
    if (matches!(returns, ReturnType::Optional(_)) || returns.inner() == ReturnType::Boolean)
        && !fails_for_zero(using, name, &zero)
    {
        return None;
    }
    Some(zero)
}

/// Whether `using` is false or empty when `name` is `zero`, whatever else it references.
fn fails_for_zero(using: &Condition, name: &str, zero: &Literal) -> bool {
    let is_name = |e: &Expression| as_reference(e) == Some(name);
    match (using.function_name(), using.arguments()) {
        ("stringEquals" | "booleanEquals", [a, b]) => {
            [(a, b), (b, a)].iter().any(|(this, other)| {
                is_name(this)
                    && match &other.kind {
                        ExpressionKind::Literal(Literal::String(t)) if t.as_static().is_none() => {
                            false
                        }
                        ExpressionKind::Literal(other) => other != zero,
                        _ => false,
                    }
            })
        }
        // A substring of "" is always empty.
        ("substring", [input, ..]) => is_name(input),
        _ => false,
    }
}

/// Whether a template within `expression` references `name`.
fn in_template(expression: &Expression, name: &str) -> bool {
    fn literal_has(literal: &Literal, name: &str) -> bool {
        match literal {
            Literal::String(template) => template
                .segments
                .iter()
                .any(|s| matches!(s, TemplateSegment::Reference { name: n, .. } if n == name)),
            Literal::Record(members) => members.iter().any(|(_, v)| literal_has(v, name)),
            Literal::Tuple(items) => items.iter().any(|i| literal_has(i, name)),
            Literal::Boolean(_) | Literal::Integer(_) => false,
        }
    }
    match &expression.kind {
        ExpressionKind::Literal(literal) => literal_has(literal, name),
        ExpressionKind::Reference(_) => false,
        ExpressionKind::Function { arguments, .. } => {
            arguments.iter().any(|a| in_template(a, name))
        }
    }
}

/// Replaces every reference to `name` outside templates with `replacement`.
fn replace_reference(expression: &Expression, name: &str, replacement: &Expression) -> Expression {
    match &expression.kind {
        ExpressionKind::Reference(n) if n == name => replacement.clone(),
        ExpressionKind::Function { name: f, arguments } => call(
            f,
            arguments
                .iter()
                .map(|a| replace_reference(a, name, replacement))
                .collect(),
            &expression.location,
        ),
        _ => expression.clone(),
    }
}

/// Java's `VariableAnalysis`, as `SsaTransform` uses it.
struct Analysis {
    /// For each name assigned more than once, the name to use for each expression it is
    /// assigned, keyed by the expression's Java text.
    unique_names: HashMap<String, HashMap<String, String>>,
}

impl Analysis {
    fn new(rules: &[Rule]) -> Self {
        // Assigned expressions, assignment counts, and referenced names, per assigned name.
        let mut expressions: HashMap<String, BTreeSet<String>> = HashMap::new();
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut uses: HashMap<String, BTreeSet<String>> = HashMap::new();
        let mut stack: Vec<&Rule> = rules.iter().collect();
        while let Some(rule) = stack.pop() {
            for condition in rule.conditions.iter() {
                if let Some(name) = &condition.assign {
                    let fn_key = key(&condition.function);
                    expressions.entry(name.clone()).or_default().insert(fn_key);
                    *counts.entry(name.clone()).or_default() += 1;
                    let names = reference_set(&condition.function)
                        .into_iter()
                        .map(str::to_owned);
                    uses.entry(name.clone()).or_default().extend(names);
                }
            }
            if let RuleKind::Tree(children) = &rule.kind {
                stack.extend(children.iter());
            }
        }

        let assigned_more_than_once = |name: &String| counts[name] > 1;
        let mut needs_ssa: HashSet<String> = expressions
            .iter()
            .filter(|(name, exprs)| assigned_more_than_once(name) && exprs.len() > 1)
            .map(|(name, _)| name.clone())
            .collect();
        // A name assigned more than once that uses a renamed name needs renaming too.
        loop {
            let more: Vec<String> = expressions
                .keys()
                .filter(|name| !needs_ssa.contains(*name) && assigned_more_than_once(name))
                .filter(|name| uses[*name].iter().any(|used| needs_ssa.contains(used)))
                .cloned()
                .collect();
            if more.is_empty() {
                break;
            }
            needs_ssa.extend(more);
        }

        let unique_names = needs_ssa
            .into_iter()
            .map(|name| {
                // `BTreeSet` iterates in Java's `String::compareTo` order.
                let names = expressions[&name]
                    .iter()
                    .enumerate()
                    .map(|(i, expression)| (expression.clone(), format!("{name}_ssa_{}", i + 1)))
                    .collect();
                (name, names)
            })
            .collect();
        Self { unique_names }
    }
}

/// The SSA renaming.
struct Ssa {
    analysis: Analysis,
    /// Name mappings in scope; the first scope maps each parameter to itself.
    scopes: Vec<HashMap<String, String>>,
    /// The (renamed) expression each unique name was given to.
    unique_expressions: HashMap<String, String>,
    counters: HashMap<String, usize>,
}

impl Ssa {
    fn new(rule_set: &EndpointRuleSet) -> Self {
        let parameters = rule_set
            .parameters
            .iter()
            .map(|p| (p.name.clone(), p.name.clone()))
            .collect();
        Self {
            analysis: Analysis::new(&rule_set.rules),
            scopes: vec![parameters],
            unique_expressions: HashMap::new(),
            counters: HashMap::new(),
        }
    }

    fn scope(&self) -> &HashMap<String, String> {
        self.scopes.last().expect("the parameter scope")
    }

    fn rules(&mut self, rules: &[Rule]) -> Result<Box<[Rule]>, CompileError> {
        rules.iter().map(|rule| self.rule(rule)).collect()
    }

    fn rule(&mut self, rule: &Rule) -> Result<Rule, CompileError> {
        self.scopes.push(self.scope().clone());
        let conditions = rule
            .conditions
            .iter()
            .map(|c| self.condition(c))
            .collect::<Result<_, _>>();
        let kind = match &rule.kind {
            RuleKind::Tree(children) => self.rules(children).map(RuleKind::Tree),
            kind => {
                let scope = self.scope();
                Ok(rename_result(kind, &|n: &str| scope.get(n).cloned()))
            }
        };
        self.scopes.pop();
        Ok(with_conditions(rule, conditions?, kind?))
    }

    fn condition(&mut self, condition: &Condition) -> Result<Condition, CompileError> {
        let unique = condition.assign.as_ref().and_then(|name| {
            let names = self.analysis.unique_names.get(name)?;
            names.get(&key(&condition.function)).cloned()
        });
        let renamed_references = reference_set(&condition.function)
            .into_iter()
            .any(|name| self.scope().get(name).is_some_and(|n| n != name));
        let Some(mut unique) = unique else {
            if !renamed_references {
                return Ok(condition.clone());
            }
            let scope = self.scope();
            let function = rename(&condition.function, &|n: &str| scope.get(n).cloned());
            return Ok(with_function(condition, function, condition.assign.clone()));
        };
        let scope = self.scope();
        let function = rename(&condition.function, &|n: &str| scope.get(n).cloned());

        // The same expression can rename differently in different scopes; each renamed
        // expression gets its own name.
        let name = condition
            .assign
            .clone()
            .expect("only assignments are renamed");
        let renamed = key(&function);
        match self.unique_expressions.get(&unique) {
            None => {
                self.unique_expressions.insert(unique.clone(), renamed);
            }
            Some(previous) if *previous != renamed => {
                loop {
                    let counter = self.counters.entry(name.clone()).or_default();
                    *counter += 1;
                    unique = format!("{name}_ssa_{counter}");
                    if !self.unique_expressions.contains_key(&unique) {
                        break;
                    }
                }
                self.unique_expressions.insert(unique.clone(), renamed);
            }
            Some(_) => {}
        }

        let scope = self.scopes.last_mut().expect("the parameter scope");
        if let Some(existing) = scope.insert(name.clone(), unique.clone()) {
            if existing != name {
                return Err(CompileError::new(format!(
                    "`{name}` is assigned while `{existing}` is in scope"
                )));
            }
        }
        Ok(with_function(condition, function, Some(unique)))
    }
}
