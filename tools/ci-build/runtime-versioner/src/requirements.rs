/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Detection of published dependency requirements that no longer accept the current
//! version of the runtime crate they point at.
//!
//! When the release publisher injects versions into manifests, a local path dependency
//! becomes a concrete requirement such as `version = "0.63.0"`, which Cargo reads with
//! caret semantics. If that dependency later moves to an incompatible version line, the
//! already-published dependent keeps requiring the old line, and consumers resolve two
//! incompatible copies. The dependent must move to a new, unpublished version so the
//! publisher can stamp the updated requirement.
//!
//! This is a current state invariant: it compares the requirement actually published for
//! the dependent's current version against the dependency version currently in the repo.
//! It therefore doesn't depend on which release tag the audit was given, and it stays
//! correct across the decoupled smithy-rs and SDK release trains.

use crate::manifest::DependencyEdge;
use anyhow::{anyhow, Context, Error, Result};
use semver::{Version, VersionReq};
use smithy_rs_tool_common::index::{
    PublishedCrateVersion, PublishedDependency, PublishedDependencyKind as DependencyKind,
};
use std::collections::BTreeMap;

/// A published dependency requirement that doesn't accept the dependency's current version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaleRequirement {
    /// The manifest key the published requirement was declared under.
    pub alias: String,
    /// The runtime crate the requirement refers to.
    pub package: String,
    /// The published requirement, for example `^0.63.0`.
    pub requirement: String,
    /// The dependency section the requirement was declared in.
    pub kind: DependencyKind,
    /// The target the requirement was declared under, if any.
    pub target: Option<String>,
    /// The dependency's current version in this repo.
    pub current_version: Version,
}

impl StaleRequirement {
    fn describe(&self) -> String {
        let mut description = format!("{} dependency ", self.kind);
        if let Some(target) = &self.target {
            description.push_str(&format!("(target {target}) "));
        }
        description.push_str(&self.package);
        if self.alias != self.package {
            description.push_str(&format!(" (renamed to {})", self.alias));
        }
        description.push_str(&format!(
            " {} does not accept the current version {}",
            self.requirement, self.current_version
        ));
        description
    }
}

/// Finds the published dependency requirements of `published` that don't accept the
/// current version of the runtime crate they refer to.
///
/// `current_edges` are the dependency entries in the dependent's current manifest, and
/// `current_versions` maps runtime crate package names to their current versions.
///
/// Returns an error, rather than a finding, when a requirement can't be evaluated at all.
pub fn stale_requirements(
    dependent: &str,
    published: &PublishedCrateVersion,
    current_edges: &[DependencyEdge],
    current_versions: &BTreeMap<String, Version>,
) -> Result<Vec<StaleRequirement>> {
    let mut stale = Vec::new();
    for requirement in &published.dependencies {
        // A dependency moving to an incompatible line doesn't require republishing a
        // crate just to update its tests and examples.
        if requirement.kind == DependencyKind::Dev {
            continue;
        }

        let Some(current_edge) = current_edge_for(requirement, current_edges)? else {
            continue;
        };

        let Some(current_version) = current_versions.get(&current_edge.package) else {
            // Third-party crates and generated SDK clients aren't managed here.
            continue;
        };

        // Checked before the `path` test below: an inherited dependency can't also declare
        // a `path`, so testing for a path first would silently skip these instead of
        // failing closed.
        if let Some(unsupported) = &current_edge.unsupported_form {
            return Err(anyhow!(
                "{dependent}'s dependency `{alias}` on the runtime crate {package} \
                 {unsupported}, so the audit can't tell whether the published \
                 requirement {requirement} is still valid",
                alias = current_edge.alias,
                package = current_edge.package,
                requirement = requirement.requirement,
            ));
        }

        // Only entries that follow the local runtime crate are checked. A version-only
        // entry is an intentional pin to a published line.
        if !current_edge.has_path {
            continue;
        }

        let parsed = VersionReq::parse(&requirement.requirement).with_context(|| {
            format!(
                "failed to parse the requirement '{requirement}' that published \
                 {dependent} {version} declares for {package}",
                requirement = requirement.requirement,
                version = published.version,
                package = requirement.package,
            )
        })?;
        if !parsed.matches(current_version) {
            stale.push(StaleRequirement {
                alias: requirement.alias.clone(),
                package: current_edge.package.clone(),
                requirement: requirement.requirement.clone(),
                kind: requirement.kind,
                target: requirement.target.clone(),
                current_version: current_version.clone(),
            });
        }
    }
    Ok(stale)
}

/// Finds the current manifest entry corresponding to a published requirement.
///
/// The manifest key, dependency kind, and target identify an exact entry. That exact
/// match is authoritative: a manifest can declare the same package both as a path
/// dependency and, under a different key, as a version-only pin to an older line, and
/// those two entries must be evaluated independently.
///
/// When the published key no longer exists at all, fall back to a current path entry for
/// the same package so that renaming or moving an entry can't hide a stale requirement.
fn current_edge_for<'a>(
    requirement: &PublishedDependency,
    current_edges: &'a [DependencyEdge],
) -> Result<Option<&'a DependencyEdge>> {
    let exact = current_edges.iter().find(|edge| {
        edge.alias == requirement.alias
            && edge.kind == requirement.kind
            && edge.target == requirement.target
    });
    if let Some(exact) = exact {
        return Ok(Some(exact));
    }

    let candidates: Vec<&DependencyEdge> = current_edges
        .iter()
        .filter(|edge| {
            edge.propagates_bumps() && edge.has_path && edge.package == requirement.package
        })
        .collect();
    if candidates.is_empty() {
        return Ok(None);
    }
    for narrowed in [
        // Prefer an entry in the same place the published requirement came from.
        candidates
            .iter()
            .filter(|edge| edge.kind == requirement.kind && edge.target == requirement.target)
            .collect::<Vec<_>>(),
        candidates
            .iter()
            .filter(|edge| edge.kind == requirement.kind)
            .collect::<Vec<_>>(),
        candidates.iter().collect::<Vec<_>>(),
    ] {
        if narrowed.len() == 1 {
            return Ok(Some(narrowed[0]));
        }
    }
    Err(anyhow!(
        "the published {kind} dependency `{alias}` on {package} has no entry with that \
         name in the current manifest, and there are multiple current path dependencies \
         on {package} ({aliases}), so the audit can't tell which one replaced it",
        kind = requirement.kind,
        alias = requirement.alias,
        package = requirement.package,
        aliases = candidates
            .iter()
            .map(|edge| edge.alias.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    ))
}

/// Builds the error reported for a dependent crate that has stale published requirements.
pub fn stale_requirements_error(
    dependent: &str,
    published: &PublishedCrateVersion,
    stale: &[StaleRequirement],
) -> Error {
    let yanked = if published.yanked { " (yanked)" } else { "" };
    let findings = stale
        .iter()
        .map(|requirement| format!("  {}", requirement.describe()))
        .collect::<Vec<_>>()
        .join("\n");
    anyhow!(
        "{dependent} {version}{yanked} has already been published, but its published \
         dependency requirements are stale:\n{findings}\nChoose a new, unpublished \
         {dependent} version so that the updated requirements can be published, then \
         rerun this audit.",
        version = published.version,
    )
}

#[cfg(test)]
mod test {
    use super::{stale_requirements, stale_requirements_error, StaleRequirement};
    use crate::manifest::DependencyEdge;
    use semver::Version;
    use smithy_rs_tool_common::index::{
        PublishedCrateVersion, PublishedDependency, PublishedDependencyKind as DependencyKind,
    };
    use std::collections::BTreeMap;

    fn path_edge(alias: &str, kind: DependencyKind) -> DependencyEdge {
        DependencyEdge {
            alias: alias.into(),
            package: alias.into(),
            kind,
            target: None,
            optional: false,
            has_path: true,
            unsupported_form: None,
        }
    }

    fn version_only_edge(alias: &str, package: &str) -> DependencyEdge {
        DependencyEdge {
            has_path: false,
            package: package.into(),
            ..path_edge(alias, DependencyKind::Normal)
        }
    }

    fn requirement(alias: &str, req: &str, kind: DependencyKind) -> PublishedDependency {
        PublishedDependency {
            alias: alias.into(),
            package: alias.into(),
            requirement: req.into(),
            kind,
            target: None,
            optional: false,
        }
    }

    fn publish(version: &str, dependencies: Vec<PublishedDependency>) -> PublishedCrateVersion {
        PublishedCrateVersion {
            version: version.into(),
            yanked: false,
            dependencies,
        }
    }

    fn current_versions(versions: &[(&str, &str)]) -> BTreeMap<String, Version> {
        versions
            .iter()
            .map(|(name, version)| (name.to_string(), Version::parse(version).unwrap()))
            .collect()
    }

    /// Evaluates a single normal path dependency on `aws-smithy-json`.
    fn json_case(published_requirement: &str, current: &str) -> Vec<StaleRequirement> {
        stale_requirements(
            "aws-config",
            &publish(
                "1.12.0",
                vec![requirement(
                    "aws-smithy-json",
                    published_requirement,
                    DependencyKind::Normal,
                )],
            ),
            &[path_edge("aws-smithy-json", DependencyKind::Normal)],
            &current_versions(&[("aws-smithy-json", current)]),
        )
        .unwrap()
    }

    /// 1. A compatible patch update doesn't require a dependent bump.
    #[test]
    fn compatible_patch_update_is_not_stale() {
        assert_eq!(
            Vec::<StaleRequirement>::new(),
            json_case("^0.63.0", "0.63.1")
        );
    }

    /// 2. The motivating incident: `aws-smithy-json` moved from `0.63.x` to `0.64.x`.
    #[test]
    fn incompatible_zero_x_update_is_stale() {
        let stale = json_case("^0.63.0", "0.64.0");
        assert_eq!(
            vec![StaleRequirement {
                alias: "aws-smithy-json".into(),
                package: "aws-smithy-json".into(),
                requirement: "^0.63.0".into(),
                kind: DependencyKind::Normal,
                target: None,
                current_version: Version::new(0, 64, 0),
            }],
            stale
        );
    }

    /// 3. A revert is not automatically compatible, and the diagnostic must not assume
    ///    the dependency moved forward.
    #[test]
    fn downgraded_dependency_is_stale() {
        let stale = json_case("^0.64.0", "0.63.1");
        assert_eq!(1, stale.len());
        let described = stale[0].describe();
        assert_eq!(
            "normal dependency aws-smithy-json ^0.64.0 does not accept the current version 0.63.1",
            described
        );
        for forward_only in ["bumped", "upgraded", "newer", "increased"] {
            assert!(
                !described.contains(forward_only),
                "diagnostic should be direction-neutral: {described}"
            );
        }
    }

    /// 4 and 5. `1.x` crates use the major version as the breaking axis.
    #[test]
    fn one_x_compatibility_follows_caret_semantics() {
        assert_eq!(Vec::<StaleRequirement>::new(), json_case("^1.6.0", "1.7.0"));
        assert_eq!(1, json_case("^1.6.0", "2.0.0").len());
    }

    /// 6. Requirement syntax is delegated to `VersionReq`.
    #[test]
    fn requirement_syntax_is_delegated_to_version_req() {
        let cases: &[(&str, &str, bool)] = &[
            ("=1.6.0", "1.6.0", false),
            ("=1.6.0", "1.6.1", true),
            ("~0.63.0", "0.63.5", false),
            ("~0.63.0", "0.64.0", true),
            ("0.63.*", "0.63.9", false),
            ("0.63.*", "0.64.0", true),
            ("*", "3.0.0", false),
            (">=1.0.0, <1.5.0", "1.4.0", false),
            (">=1.0.0, <1.5.0", "1.6.0", true),
            ("^1.0.0-alpha.1", "1.0.0-alpha.2", false),
            ("^1.0.0", "2.0.0-alpha.1", true),
        ];
        for (req, current, expect_stale) in cases {
            let stale = json_case(req, current);
            assert_eq!(
                *expect_stale,
                !stale.is_empty(),
                "expected stale={expect_stale} for requirement {req} against {current}"
            );
        }
    }

    /// 7. A published record's requirements are evaluated against the dependency's current
    ///    version, not the dependent's, so a dependent that has moved on is only exempt
    ///    because the caller skips it.
    ///
    ///    `audit_published_requirements` looks the record up by the dependent's current
    ///    version and does nothing when there is none, which the
    ///    `bumping_the_dependent_resolves_a_stale_requirement` integration test covers
    ///    end to end. This test pins the remaining half of that contract: an old published
    ///    record still produces findings when it is the one handed in.
    #[test]
    fn requirements_are_evaluated_for_the_record_handed_in() {
        let stale = stale_requirements(
            "aws-config",
            &publish(
                "1.11.0",
                vec![requirement(
                    "aws-smithy-json",
                    "^0.60.0",
                    DependencyKind::Normal,
                )],
            ),
            &[path_edge("aws-smithy-json", DependencyKind::Normal)],
            &current_versions(&[
                ("aws-smithy-json", "0.61.0"),
                // The dependent's own current version is irrelevant here.
                ("aws-config", "1.12.0"),
            ]),
        )
        .unwrap();
        assert_eq!(1, stale.len());
        assert_eq!("aws-smithy-json", stale[0].package);
    }

    /// 8. A yanked version is still published and its number can't be reused, so its
    ///    requirements are still evaluated.
    #[test]
    fn yanked_published_version_is_still_evaluated() {
        let mut published = publish(
            "1.12.0",
            vec![requirement(
                "aws-smithy-json",
                "^0.63.0",
                DependencyKind::Normal,
            )],
        );
        published.yanked = true;
        let stale = stale_requirements(
            "aws-config",
            &published,
            &[path_edge("aws-smithy-json", DependencyKind::Normal)],
            &current_versions(&[("aws-smithy-json", "0.64.0")]),
        )
        .unwrap();
        assert_eq!(1, stale.len());
        let rendered = format!(
            "{:#}",
            stale_requirements_error("aws-config", &published, &stale)
        );
        assert!(
            rendered.contains("1.12.0 (yanked)"),
            "yanked state should be reported: {rendered}"
        );
    }

    /// 9. Dev requirements are ignored.
    #[test]
    fn dev_requirement_is_ignored() {
        let stale = stale_requirements(
            "aws-config",
            &publish(
                "1.12.0",
                vec![requirement(
                    "aws-smithy-protocol-test",
                    "^0.63.0",
                    DependencyKind::Dev,
                )],
            ),
            &[path_edge("aws-smithy-protocol-test", DependencyKind::Dev)],
            &current_versions(&[("aws-smithy-protocol-test", "0.64.0")]),
        )
        .unwrap();
        assert_eq!(Vec::<StaleRequirement>::new(), stale);
    }

    /// 10. Optional normal and build requirements are enforced.
    #[test]
    fn optional_and_build_requirements_are_enforced() {
        let mut optional = requirement("aws-smithy-mocks", "^0.3.0", DependencyKind::Normal);
        optional.optional = true;
        let mut optional_edge = path_edge("aws-smithy-mocks", DependencyKind::Normal);
        optional_edge.optional = true;

        let stale = stale_requirements(
            "aws-config",
            &publish(
                "1.12.0",
                vec![
                    optional,
                    requirement("aws-smithy-types", "^1.6.0", DependencyKind::Build),
                ],
            ),
            &[
                optional_edge,
                path_edge("aws-smithy-types", DependencyKind::Build),
            ],
            &current_versions(&[("aws-smithy-mocks", "0.4.0"), ("aws-smithy-types", "2.0.0")]),
        )
        .unwrap();
        assert_eq!(
            vec!["aws-smithy-mocks", "aws-smithy-types"],
            stale.iter().map(|s| s.package.as_str()).collect::<Vec<_>>()
        );
        assert_eq!(DependencyKind::Build, stale[1].kind);
    }

    /// 11. Target-specific requirements are enforced and the target is reported.
    #[test]
    fn target_specific_requirement_reports_its_target() {
        let mut published_requirement =
            requirement("aws-smithy-async", "^1.2.0", DependencyKind::Normal);
        published_requirement.target = Some("cfg(windows)".into());
        let mut edge = path_edge("aws-smithy-async", DependencyKind::Normal);
        edge.target = Some("cfg(windows)".into());

        let stale = stale_requirements(
            "aws-config",
            &publish("1.12.0", vec![published_requirement]),
            &[edge],
            &current_versions(&[("aws-smithy-async", "2.0.0")]),
        )
        .unwrap();
        assert_eq!(Some("cfg(windows)"), stale[0].target.as_deref());
        assert!(
            stale[0].describe().contains("(target cfg(windows))"),
            "target should be reported: {}",
            stale[0].describe()
        );
    }

    /// 12. A renamed dependency is resolved to the package it actually refers to.
    #[test]
    fn renamed_dependency_resolves_to_its_package() {
        let mut published_requirement =
            requirement("json-alias", "^0.63.0", DependencyKind::Normal);
        published_requirement.package = "aws-smithy-json".into();
        let mut edge = path_edge("json-alias", DependencyKind::Normal);
        edge.package = "aws-smithy-json".into();

        let stale = stale_requirements(
            "aws-config",
            &publish("1.12.0", vec![published_requirement]),
            &[edge],
            &current_versions(&[("aws-smithy-json", "0.64.0")]),
        )
        .unwrap();
        assert_eq!("aws-smithy-json", stale[0].package);
        assert_eq!("json-alias", stale[0].alias);
        assert!(
            stale[0].describe().contains("(renamed to json-alias)"),
            "rename should be reported: {}",
            stale[0].describe()
        );
    }

    /// 13. The `aws-smithy-http-server-metrics` topology: a current path dependency and a
    ///     renamed version-only pin on the same package must be evaluated independently.
    #[test]
    fn version_only_compatibility_pin_is_ignored() {
        let mut pinned = requirement(
            "aws-smithy-http-server-065",
            "^0.65",
            DependencyKind::Normal,
        );
        pinned.package = "aws-smithy-http-server".into();
        let current = requirement("aws-smithy-http-server", "^0.67.1", DependencyKind::Normal);

        let edges = [
            path_edge("aws-smithy-http-server", DependencyKind::Normal),
            version_only_edge("aws-smithy-http-server-065", "aws-smithy-http-server"),
        ];

        // The current path dependency is satisfied, and the pin must not be evaluated
        // against the current version.
        assert_eq!(
            Vec::<StaleRequirement>::new(),
            stale_requirements(
                "aws-smithy-http-server-metrics",
                &publish("0.2.2", vec![current.clone(), pinned.clone()]),
                &edges,
                &current_versions(&[("aws-smithy-http-server", "0.67.1")]),
            )
            .unwrap()
        );

        // The path dependency is still enforced.
        let stale = stale_requirements(
            "aws-smithy-http-server-metrics",
            &publish("0.2.2", vec![current, pinned]),
            &edges,
            &current_versions(&[("aws-smithy-http-server", "0.68.0")]),
        )
        .unwrap();
        assert_eq!(1, stale.len());
        assert_eq!("aws-smithy-http-server", stale[0].alias);
        assert_eq!("^0.67.1", stale[0].requirement);
    }

    /// 14. A published normal requirement doesn't match a current dev-only entry.
    #[test]
    fn normal_requirement_does_not_match_dev_only_entry() {
        let stale = stale_requirements(
            "aws-config",
            &publish(
                "1.12.0",
                vec![requirement(
                    "aws-smithy-json",
                    "^0.63.0",
                    DependencyKind::Normal,
                )],
            ),
            &[path_edge("aws-smithy-json", DependencyKind::Dev)],
            &current_versions(&[("aws-smithy-json", "0.64.0")]),
        )
        .unwrap();
        assert_eq!(Vec::<StaleRequirement>::new(), stale);
    }

    /// 15. Renaming or moving an entry can't hide a stale requirement.
    #[test]
    fn moved_entry_falls_back_to_the_package_path_edge() {
        let mut published_requirement =
            requirement("aws-smithy-json", "^0.63.0", DependencyKind::Normal);
        published_requirement.target = None;

        // The entry was renamed and moved under a target.
        let mut moved = path_edge("json-alias", DependencyKind::Normal);
        moved.package = "aws-smithy-json".into();
        moved.target = Some("cfg(unix)".into());

        let stale = stale_requirements(
            "aws-config",
            &publish("1.12.0", vec![published_requirement]),
            &[moved],
            &current_versions(&[("aws-smithy-json", "0.64.0")]),
        )
        .unwrap();
        assert_eq!(1, stale.len());
        assert_eq!("aws-smithy-json", stale[0].package);
    }

    /// 16. Ambiguous fallback is reported rather than guessed.
    #[test]
    fn ambiguous_fallback_is_an_error() {
        let published_requirement =
            requirement("aws-smithy-json", "^0.63.0", DependencyKind::Normal);
        let mut first = path_edge("json-one", DependencyKind::Normal);
        first.package = "aws-smithy-json".into();
        let mut second = path_edge("json-two", DependencyKind::Normal);
        second.package = "aws-smithy-json".into();

        let err = stale_requirements(
            "aws-config",
            &publish("1.12.0", vec![published_requirement]),
            &[first, second],
            &current_versions(&[("aws-smithy-json", "0.64.0")]),
        )
        .expect_err("two candidate path edges are ambiguous");
        let err = format!("{err:#}");
        assert!(
            err.contains("json-one, json-two"),
            "unexpected error: {err}"
        );
        assert!(
            err.contains("can't tell which one replaced it"),
            "unexpected error: {err}"
        );
    }

    /// 17. Third-party crates and generated SDK clients aren't managed runtime crates.
    #[test]
    fn unmanaged_dependency_is_ignored() {
        let stale = stale_requirements(
            "aws-config",
            &publish(
                "1.12.0",
                vec![
                    requirement("aws-sdk-sts", "^1.114.0", DependencyKind::Normal),
                    requirement("tokio", "^1.20.1", DependencyKind::Normal),
                ],
            ),
            &[
                path_edge("aws-sdk-sts", DependencyKind::Normal),
                path_edge("tokio", DependencyKind::Normal),
            ],
            &current_versions(&[("aws-smithy-json", "0.64.0")]),
        )
        .unwrap();
        assert_eq!(Vec::<StaleRequirement>::new(), stale);
    }

    /// 18. Every stale edge for a dependent is reported together.
    #[test]
    fn multiple_stale_requirements_are_aggregated() {
        let stale = stale_requirements(
            "aws-config",
            &publish(
                "1.12.0",
                vec![
                    requirement("aws-smithy-json", "^0.63.0", DependencyKind::Normal),
                    requirement("aws-smithy-schema", "^0.2.0", DependencyKind::Normal),
                    requirement("aws-smithy-types", "^1.6.0", DependencyKind::Normal),
                ],
            ),
            &[
                path_edge("aws-smithy-json", DependencyKind::Normal),
                path_edge("aws-smithy-schema", DependencyKind::Normal),
                path_edge("aws-smithy-types", DependencyKind::Normal),
            ],
            &current_versions(&[
                ("aws-smithy-json", "0.64.0"),
                ("aws-smithy-schema", "0.4.0"),
                // compatible, so not reported
                ("aws-smithy-types", "1.7.0"),
            ]),
        )
        .unwrap();
        assert_eq!(2, stale.len());

        let rendered = format!(
            "{:#}",
            stale_requirements_error("aws-config", &publish("1.12.0", vec![]), &stale)
        );
        assert!(rendered.contains("aws-smithy-json ^0.63.0"), "{rendered}");
        assert!(rendered.contains("aws-smithy-schema ^0.2.0"), "{rendered}");
        assert!(
            rendered.contains("Choose a new, unpublished aws-config version"),
            "{rendered}"
        );
    }

    /// 19. A requirement that can't be parsed fails closed, with context.
    #[test]
    fn malformed_requirement_fails_closed() {
        let err = stale_requirements(
            "aws-config",
            &publish(
                "1.12.0",
                vec![requirement(
                    "aws-smithy-json",
                    "not-a-requirement",
                    DependencyKind::Normal,
                )],
            ),
            &[path_edge("aws-smithy-json", DependencyKind::Normal)],
            &current_versions(&[("aws-smithy-json", "0.64.0")]),
        )
        .expect_err("'not-a-requirement' can't be parsed");
        let err = format!("{err:#}");
        assert!(err.contains("aws-config 1.12.0"), "unexpected error: {err}");
        assert!(err.contains("not-a-requirement"), "unexpected error: {err}");
    }

    /// Dependency forms the audit can't reason about fail closed when they refer to a
    /// managed runtime crate.
    #[test]
    fn unsupported_managed_dependency_form_fails_closed() {
        // Parsed from a manifest rather than hand-built: Cargo forbids combining
        // `workspace = true` with `path`, so testing for a path first would skip these
        // entries instead of failing closed.
        let edges = crate::manifest::dependency_edges(
            &toml::from_str(
                r#"
                [dependencies]
                aws-smithy-json = { workspace = true }
                "#,
            )
            .unwrap(),
        )
        .unwrap();
        assert!(
            !edges[0].has_path,
            "an inherited entry can't declare a path"
        );

        let published = publish(
            "1.12.0",
            vec![requirement(
                "aws-smithy-json",
                "^0.63.0",
                DependencyKind::Normal,
            )],
        );

        let err = stale_requirements(
            "aws-config",
            &published,
            &edges,
            &current_versions(&[("aws-smithy-json", "0.64.0")]),
        )
        .expect_err("workspace inheritance can't be evaluated");
        let err = format!("{err:#}");
        assert!(
            err.contains("uses workspace dependency inheritance"),
            "unexpected error: {err}"
        );
        assert!(err.contains("aws-smithy-json"), "unexpected error: {err}");

        // The same form on a crate that isn't a managed runtime crate is not an error.
        assert_eq!(
            Vec::<StaleRequirement>::new(),
            stale_requirements("aws-config", &published, &edges, &current_versions(&[])).unwrap()
        );
    }
}
