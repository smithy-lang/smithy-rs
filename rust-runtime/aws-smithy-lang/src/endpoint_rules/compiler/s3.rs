/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Smithy Java's `S3TreeRewriter` (`smithy-aws-endpoints` 1.73.0) with its default
//! transforms, which `CompileBddForAws` applies to S3's rule set before compiling it.
//!
//! Many S3 Express endpoints differ only in FIPS, dual-stack, and auth-scheme spellings, chosen
//! by enclosing conditions. The rewrite spells those parts with variables assigned by `ite`
//! conditions, so the endpoints become equal and the BDD shares them:
//!
//! - `S3AzCanonicalizerTransform` computes the availability zone ID from a bucket name with
//!   `split` instead of a `substring` whose offsets vary between rules.
//! - `S3ExpressEndpointTransform` rewrites S3 Express URLs (via `S3ExpressUrlCanonicalizer`)
//!   and the `sigv4`/`sigv4-s3express` auth-scheme names.
//!
//! Like Java's, the rewrite assumes S3's rule structure: an endpoint's `-fips` spelling, for
//! example, is only correct to replace if the endpoint is reached only when `UseFIPS` is set.
//! Java checks this by running S3's endpoint tests on the rewritten rules; that needs rule
//! evaluation, which this crate does not do.
//!
//! Differences from Java, which change nothing for S3's rule set:
//! - The availability zone rewrite introduces a negative `getAttr` index, which needs version
//!   1.1. Java leaves a 1.0 rule set at 1.0; this raises it.
//! - Java fails on a rewritten URL that is not a valid template, which happens when the matched
//!   text is inside a template reference. This leaves such an endpoint unchanged.
//! - Java's patterns stop at line terminators and may drop one at the end of a URL. This leaves
//!   URLs containing one unchanged.

use super::syntax::{as_call, as_reference, call, get_attr, literal, reference};
use crate::endpoint_rules::codec::{encode_template, parse_template};
use crate::endpoint_rules::language::{
    Condition, EndpointDefinition, Expression, ExpressionKind, Literal, Location, PathSegment,
    Rule, RuleKind, RulesVersion,
};
use crate::endpoint_rules::EndpointRuleSet;

/// The service whose rule set `CompileBddForAws` rewrites.
pub(super) const SERVICE: &str = "com.amazonaws.s3#AmazonS3";

const VAR_FIPS: &str = "_s3e_fips";
const VAR_DS: &str = "_s3e_ds";
const VAR_AUTH: &str = "_s3e_auth";
const AUTH_SIGV4: &str = "sigv4";
const AUTH_SIGV4_S3EXPRESS: &str = "sigv4-s3express";

/// Rewrites S3's rule set as Java's `S3TreeRewriter.transform` does.
pub(super) fn rewrite(rule_set: &EndpointRuleSet) -> EndpointRuleSet {
    let mut rewrote_az = false;
    let rules = rule_set
        .rules
        .iter()
        .map(|rule| rewrite_rule(rule, &mut rewrote_az))
        .collect();
    EndpointRuleSet {
        version: if rewrote_az {
            rule_set.version.max(RulesVersion::V1_1)
        } else {
            rule_set.version
        },
        parameters: rule_set.parameters.clone(),
        rules,
    }
}

/// Recurses as deep as the rule tree, which decoding limits.
fn rewrite_rule(rule: &Rule, rewrote_az: &mut bool) -> Rule {
    let conditions: Vec<Condition> = rule
        .conditions
        .iter()
        .map(|condition| match canonical_az(condition) {
            Some(rewritten) => {
                *rewrote_az = true;
                rewritten
            }
            None => condition.clone(),
        })
        .collect();
    let kind = match &rule.kind {
        RuleKind::Tree(rules) => RuleKind::Tree(
            rules
                .iter()
                .map(|rule| rewrite_rule(rule, rewrote_az))
                .collect(),
        ),
        RuleKind::Endpoint(endpoint) => {
            if let Some(rewritten) = rewrite_endpoint(&conditions, endpoint, &rule.location) {
                return rewritten;
            }
            RuleKind::Endpoint(endpoint.clone())
        }
        RuleKind::Error(error) => RuleKind::Error(error.clone()),
    };
    Rule {
        conditions: conditions.into_boxed_slice(),
        documentation: rule.documentation.clone(),
        kind,
        location: rule.location.clone(),
    }
}

/// `S3AzCanonicalizerTransform`: `s3expressAvailabilityZoneId = substring(Bucket, ...)`
/// becomes `s3expressAvailabilityZoneId = getAttr(split(Bucket, "--", 0), "[-2]")`.
fn canonical_az(condition: &Condition) -> Option<Condition> {
    if condition.assign.as_deref() != Some("s3expressAvailabilityZoneId") {
        return None;
    }
    let (name, arguments) = as_call(&condition.function)?;
    if name != "substring" || arguments.first().and_then(as_reference) != Some("Bucket") {
        return None;
    }
    let location = &condition.location;
    let split = call(
        "split",
        vec![
            reference("Bucket", location),
            string("--", location),
            literal(Literal::Integer(0), location),
        ],
        location,
    );
    Some(Condition {
        function: get_attr(split, &[PathSegment::Index(-2)], location),
        ..condition.clone()
    })
}

/// `S3ExpressEndpointTransform.endpointRule`: the rewritten rule, or `None` to keep it.
///
/// Like Java, a rewritten rule has no documentation.
fn rewrite_endpoint(
    conditions: &[Condition],
    endpoint: &EndpointDefinition,
    location: &Location,
) -> Option<Rule> {
    let ExpressionKind::Literal(Literal::String(url)) = &endpoint.url.kind else {
        return None;
    };
    let url = encode_template(url);
    let express_url = url.contains("s3express");
    let express_backend = endpoint
        .properties
        .iter()
        .find(|(key, _)| key == "backend")
        .and_then(|(_, value)| static_string(value))
        .is_some_and(|backend| java_equals_ignore_case(backend, "S3Express"));
    let mut conditions = conditions.to_vec();
    let (url, properties) = if express_url {
        let (canonical, bucket) = canonical_url(&url)?;
        let canonical = parse_template(&canonical).ok()?;
        conditions.push(ite(
            VAR_FIPS,
            reference("UseFIPS", location),
            "-fips",
            location,
        ));
        conditions.push(ite(
            VAR_DS,
            reference("UseDualStack", location),
            ".dualstack",
            location,
        ));
        conditions.push(auth_ite(location));
        let properties = bucket
            .then(|| canonical_auth_schemes(&endpoint.properties))
            .flatten()
            .unwrap_or_else(|| endpoint.properties.clone());
        let url = literal(Literal::String(canonical), &endpoint.url.location);
        (url, properties)
    } else if express_backend {
        let properties = canonical_auth_schemes(&endpoint.properties)?;
        conditions.push(auth_ite(location));
        (endpoint.url.clone(), properties)
    } else {
        return None;
    };
    Some(Rule {
        conditions: conditions.into_boxed_slice(),
        documentation: None,
        kind: RuleKind::Endpoint(EndpointDefinition {
            url,
            headers: endpoint.headers.clone(),
            properties,
            location: endpoint.location.clone(),
        }),
        location: location.clone(),
    })
}

/// `assign = ite(condition, true_value, "")`.
fn ite(assign: &str, condition: Expression, true_value: &str, location: &Location) -> Condition {
    ite_strings(assign, condition, true_value, "", location)
}

fn ite_strings(
    assign: &str,
    condition: Expression,
    true_value: &str,
    false_value: &str,
    location: &Location,
) -> Condition {
    let arguments = vec![
        condition,
        string(true_value, location),
        string(false_value, location),
    ];
    Condition {
        function: call("ite", arguments, location),
        assign: Some(assign.to_owned()),
        location: location.clone(),
    }
}

/// `_s3e_auth = ite(coalesce(DisableS3ExpressSessionAuth, false), "sigv4", "sigv4-s3express")`.
fn auth_ite(location: &Location) -> Condition {
    let disabled = call(
        "coalesce",
        vec![
            reference("DisableS3ExpressSessionAuth", location),
            literal(Literal::Boolean(false), location),
        ],
        location,
    );
    ite_strings(
        VAR_AUTH,
        disabled,
        AUTH_SIGV4,
        AUTH_SIGV4_S3EXPRESS,
        location,
    )
}

/// A string literal without references.
fn string(value: &str, location: &Location) -> Expression {
    let template = parse_template(value).expect("the rewrite's strings have no braces");
    literal(Literal::String(template), location)
}

fn static_string(value: &Literal) -> Option<&str> {
    match value {
        Literal::String(template) => template.as_static(),
        _ => None,
    }
}

/// `canonicalizeAuthScheme`: names each `sigv4` or `sigv4-s3express` auth scheme
/// `{_s3e_auth}`, or `None` if there are none.
fn canonical_auth_schemes(properties: &[(String, Literal)]) -> Option<Box<[(String, Literal)]>> {
    let at = properties
        .iter()
        .position(|(key, _)| key == "authSchemes")?;
    let Literal::Tuple(schemes) = &properties[at].1 else {
        return None;
    };
    let mut changed = false;
    let schemes = schemes
        .iter()
        .map(|scheme| {
            let Literal::Record(fields) = scheme else {
                return scheme.clone();
            };
            let Some(name) = fields.iter().position(|(key, _)| key == "name") else {
                return scheme.clone();
            };
            if !matches!(
                static_string(&fields[name].1),
                Some(AUTH_SIGV4 | AUTH_SIGV4_S3EXPRESS)
            ) {
                return scheme.clone();
            }
            changed = true;
            let mut fields = fields.to_vec();
            let auth = parse_template(&format!("{{{VAR_AUTH}}}")).expect("a valid reference");
            fields[name].1 = Literal::String(auth);
            Literal::Record(fields.into_boxed_slice())
        })
        .collect();
    if !changed {
        return None;
    }
    let mut properties = properties.to_vec();
    properties[at].1 = Literal::Tuple(schemes);
    Some(properties.into_boxed_slice())
}

/// Java's `String.equalsIgnoreCase` against an ASCII `expected`: besides ASCII case, Java
/// matches `ſ` (long s), whose uppercase is `S`. No other character uppercases to an ASCII
/// letter.
fn java_equals_ignore_case(actual: &str, expected: &str) -> bool {
    actual.chars().count() == expected.len()
        && actual
            .chars()
            .zip(expected.chars())
            .all(|(a, e)| a.eq_ignore_ascii_case(&e) || (a == 'ſ' && e.eq_ignore_ascii_case(&'s')))
}

/// `S3ExpressUrlCanonicalizer` patterns, tried in order: whether a bucket (rather than control
/// plane) endpoint, and whether the URL is spelled with `-fips` and `.dualstack`.
const PATTERNS: [(bool, bool, bool); 8] = [
    (false, true, true),
    (false, true, false),
    (false, false, true),
    (false, false, false),
    (true, true, true),
    (true, true, false),
    (true, false, true),
    (true, false, false),
];

/// `S3ExpressUrlCanonicalizer.canonicalize`: the URL with its FIPS and dual-stack spellings
/// replaced by `{_s3e_fips}` and `{_s3e_ds}`, and whether it is a bucket endpoint.
///
/// Java matches each pattern with `Matcher.find`, at the first position it matches, which is
/// what this does. The control patterns are `(s3express-control)(-fips)?\.(dualstack\.)?(.+)$`
/// and the bucket patterns `(s3express)-(fips-)?([^.]+)\.(dualstack\.)?(.+)$`, without
/// `dualstack\.` requiring `(?!dualstack)`.
fn canonical_url(url: &str) -> Option<(String, bool)> {
    // Java's `.` does not match these, and `$` matches before one at the end.
    if url.contains(['\n', '\r', '\u{85}', '\u{2028}', '\u{2029}']) {
        return None;
    }
    for (bucket, fips, dual_stack) in PATTERNS {
        // Occurrences can overlap (`s3expres3express`), so try every position.
        let starts = (0..url.len()).filter(|&i| url.is_char_boundary(i));
        for start in starts.filter(|&i| url[i..].starts_with("s3express")) {
            let Some((az, suffix)) = match_at(&url[start..], bucket, fips, dual_stack) else {
                continue;
            };
            let prefix = &url[..start];
            let canonical = match az {
                Some(az) => {
                    format!("{prefix}s3express{{{VAR_FIPS}}}-{az}{{{VAR_DS}}}.{suffix}")
                }
                None => format!("{prefix}s3express-control{{{VAR_FIPS}}}{{{VAR_DS}}}.{suffix}"),
            };
            return Some((canonical, bucket));
        }
    }
    None
}

/// Matches a pattern at the start of `text`, returning its availability zone (for a bucket
/// pattern) and the rest of the URL.
fn match_at(
    text: &str,
    bucket: bool,
    fips: bool,
    dual_stack: bool,
) -> Option<(Option<&str>, &str)> {
    let mut rest;
    let mut az = None;
    if bucket {
        rest = text.strip_prefix("s3express-")?;
        if fips {
            rest = rest.strip_prefix("fips-")?;
        }
        // `[^.]+` takes everything up to the next `.`.
        let end = rest.find('.').filter(|&end| end > 0)?;
        az = Some(&rest[..end]);
        rest = &rest[end..];
    } else {
        rest = text.strip_prefix("s3express-control")?;
        if fips {
            rest = rest.strip_prefix("-fips")?;
        }
    }
    rest = rest.strip_prefix('.')?;
    if dual_stack {
        rest = rest.strip_prefix("dualstack.")?;
    } else if rest.starts_with("dualstack") {
        return None;
    }
    (!rest.is_empty()).then_some((az, rest))
}

#[cfg(test)]
mod tests {
    use super::canonical_url;

    #[test]
    fn canonical_urls() {
        let canonical = canonical_url;
        let control = |s: &str| Some((s.to_owned(), false));
        let bucket = |s: &str| Some((s.to_owned(), true));
        assert_eq!(
            canonical("https://s3express-control-fips.dualstack.{Region}.x/{b}"),
            control("https://s3express-control{_s3e_fips}{_s3e_ds}.{Region}.x/{b}"),
        );
        assert_eq!(
            canonical("https://s3express-control.{Region}.x"),
            control("https://s3express-control{_s3e_fips}{_s3e_ds}.{Region}.x"),
        );
        assert_eq!(
            canonical("https://{Bucket}.s3express-fips-{az}.{Region}.x"),
            bucket("https://{Bucket}.s3express{_s3e_fips}-{az}{_s3e_ds}.{Region}.x"),
        );
        // `(?!dualstack)` only checks a prefix, so `.dualstackx` is neither spelling.
        assert_eq!(canonical("https://s3express-control.dualstackx.y"), None);
        // The FIPS bucket patterns come first, so `fips-` is not part of the zone.
        assert_eq!(
            canonical("https://s3express-fips-a.dualstack.r"),
            bucket("https://s3express{_s3e_fips}-a{_s3e_ds}.r"),
        );
        // Occurrences can overlap, and a later one can match where an earlier one does not.
        assert_eq!(
            canonical("https://s3express3express-a.r"),
            bucket("https://s3express3express{_s3e_fips}-a{_s3e_ds}.r"),
        );
        assert_eq!(canonical("https://s3express-a"), None);
        assert_eq!(canonical("https://s3express-a.r\n"), None);
    }
}
