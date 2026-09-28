/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Parsing of dependency declarations from runtime crate manifests.
//!
//! The audit needs to know, for each dependency entry in a current runtime manifest:
//!
//! - the manifest key (`alias`) it was declared under,
//! - the package it actually refers to (`package = `, when renamed),
//! - which dependency section and target it was declared in, and
//! - whether it is a local (`path = `) dependency.
//!
//! The alias, kind, and target identify a specific manifest entry so it can be matched
//! against the corresponding entry in a published crates.io index record. The package
//! name identifies which runtime crate provides the dependency's version.

use anyhow::{Context, Result};
use smithy_rs_tool_common::index::PublishedDependencyKind as DependencyKind;

/// A single dependency entry declared by a runtime crate manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencyEdge {
    /// The manifest key this dependency was declared under.
    ///
    /// For `some-crate-065 = { package = "some-crate", version = "0.65" }` this is
    /// `some-crate-065`.
    pub alias: String,
    /// The package actually depended on: `package = ` when renamed, otherwise the alias.
    pub package: String,
    /// The dependency section this entry was declared in.
    pub kind: DependencyKind,
    /// The target this entry was declared under, exactly as it appears in the manifest
    /// after TOML parsing (for example `cfg(windows)`). `None` for a top-level table.
    pub target: Option<String>,
    /// True if the entry is declared optional.
    pub optional: bool,
    /// True if the entry has a `path` key, meaning it follows the local runtime crate.
    ///
    /// The path itself is deliberately not recorded. Some runtime crates (`aws-config`)
    /// point at the generated SDK tree, which is gitignored and normally absent when the
    /// audit runs, so a path must never be resolved, canonicalized, or read from.
    pub has_path: bool,
    /// Set when the entry uses a form this audit cannot reason about, such as workspace
    /// inheritance or an alternate registry.
    pub unsupported_form: Option<String>,
}

impl DependencyEdge {
    /// True if this entry participates in dependency bump propagation.
    ///
    /// Dev dependencies do not: a dependency moving to an incompatible version line
    /// doesn't require republishing a crate just to update its tests and examples.
    pub fn propagates_bumps(&self) -> bool {
        self.kind != DependencyKind::Dev
    }
}

/// Parses every dependency entry declared by a manifest.
///
/// Dev dependencies are included so that an entry which only exists as a dev dependency
/// can be distinguished from a missing entry; use [`DependencyEdge::propagates_bumps`] to
/// filter them out.
pub fn dependency_edges(manifest: &toml::Value) -> Result<Vec<DependencyEdge>> {
    let mut edges = Vec::new();

    collect_tables(&mut edges, manifest, None)?;

    if let Some(targets) = manifest.get("target") {
        let targets = targets
            .as_table()
            .context("`[target]` must be a table of target expressions")?;
        for (target, tables) in targets {
            collect_tables(&mut edges, tables, Some(target))?;
        }
    }

    Ok(edges)
}

/// Collects the three dependency sections from either the manifest root or one target table.
fn collect_tables(
    edges: &mut Vec<DependencyEdge>,
    tables: &toml::Value,
    target: Option<&str>,
) -> Result<()> {
    for (table_name, kind) in [
        ("dependencies", DependencyKind::Normal),
        ("build-dependencies", DependencyKind::Build),
        ("dev-dependencies", DependencyKind::Dev),
    ] {
        let Some(table) = tables.get(table_name) else {
            continue;
        };
        let table = table
            .as_table()
            .with_context(|| format!("`{}` must be a table", table_path(table_name, target)))?;
        for (alias, entry) in table {
            edges.push(parse_entry(alias, entry, kind, target).with_context(|| {
                format!(
                    "failed to parse dependency `{alias}` in `{}`",
                    table_path(table_name, target)
                )
            })?);
        }
    }
    Ok(())
}

fn table_path(table_name: &str, target: Option<&str>) -> String {
    match target {
        Some(target) => format!("[target.\"{target}\".{table_name}]"),
        None => format!("[{table_name}]"),
    }
}

fn parse_entry(
    alias: &str,
    entry: &toml::Value,
    kind: DependencyKind,
    target: Option<&str>,
) -> Result<DependencyEdge> {
    let mut edge = DependencyEdge {
        alias: alias.to_string(),
        package: alias.to_string(),
        kind,
        target: target.map(ToString::to_string),
        optional: false,
        has_path: false,
        unsupported_form: None,
    };

    // A bare `foo = "1.0"` entry is a version-only registry dependency.
    if entry.as_str().is_some() {
        return Ok(edge);
    }

    let entry = entry
        .as_table()
        .context("dependency must be either a version string or a table")?;

    if let Some(package) = entry.get("package") {
        edge.package = package
            .as_str()
            .context("`package` must be a string")?
            .to_string();
    }
    edge.has_path = entry.contains_key("path");
    edge.optional = entry
        .get("optional")
        .map(|optional| optional.as_bool().context("`optional` must be a boolean"))
        .transpose()?
        .unwrap_or(false);

    // Both of these forms hide where the dependency's version requirement comes from, so
    // the audit can't tell whether it tracks the local runtime crate. Record it rather
    // than failing here; it only matters if the package turns out to be a managed crate.
    if entry
        .get("workspace")
        .and_then(|workspace| workspace.as_bool())
        .unwrap_or(false)
    {
        edge.unsupported_form = Some("uses workspace dependency inheritance".to_string());
    } else if entry.contains_key("registry") || entry.contains_key("registry-index") {
        edge.unsupported_form = Some("specifies an alternate registry".to_string());
    }

    Ok(edge)
}

#[cfg(test)]
mod test {
    use super::{dependency_edges, DependencyEdge};
    use smithy_rs_tool_common::index::PublishedDependencyKind as DependencyKind;

    fn edges(manifest: &str) -> Vec<DependencyEdge> {
        dependency_edges(&toml::from_str(manifest).unwrap()).unwrap()
    }

    fn find<'a>(edges: &'a [DependencyEdge], alias: &str) -> &'a DependencyEdge {
        edges
            .iter()
            .find(|edge| edge.alias == alias)
            .unwrap_or_else(|| panic!("no dependency edge for `{alias}`: {edges:#?}"))
    }

    #[test]
    fn top_level_normal_path_dependency() {
        let edges = edges(
            r#"
            [package]
            name = "aws-config"

            [dependencies]
            aws-smithy-json = { path = "../aws-smithy-json", version = "0.63.0" }
            "#,
        );
        assert_eq!(
            &DependencyEdge {
                alias: "aws-smithy-json".into(),
                package: "aws-smithy-json".into(),
                kind: DependencyKind::Normal,
                target: None,
                optional: false,
                has_path: true,
                unsupported_form: None,
            },
            find(&edges, "aws-smithy-json")
        );
        assert!(find(&edges, "aws-smithy-json").propagates_bumps());
    }

    #[test]
    fn optional_normal_path_dependency_is_included() {
        let edges = edges(
            r#"
            [dependencies]
            aws-smithy-mocks = { path = "../aws-smithy-mocks", version = "0.3.0", optional = true }
            "#,
        );
        let edge = find(&edges, "aws-smithy-mocks");
        assert!(edge.optional);
        assert!(edge.has_path);
        assert!(edge.propagates_bumps());
    }

    #[test]
    fn build_path_dependency_is_included() {
        let edges = edges(
            r#"
            [build-dependencies]
            aws-smithy-types = { path = "../aws-smithy-types", version = "1.7.0" }
            "#,
        );
        let edge = find(&edges, "aws-smithy-types");
        assert_eq!(DependencyKind::Build, edge.kind);
        assert!(edge.has_path);
        assert!(edge.propagates_bumps());
    }

    #[test]
    fn target_specific_dependencies_keep_their_target() {
        let edges = edges(
            r#"
            [target.'cfg(not(target_family = "wasm"))'.dependencies]
            aws-smithy-async = { path = "../aws-smithy-async", version = "1.2.0" }

            [target.'cfg(windows)'.build-dependencies]
            aws-smithy-types = { path = "../aws-smithy-types", version = "1.7.0" }
            "#,
        );
        let normal = find(&edges, "aws-smithy-async");
        assert_eq!(
            Some("cfg(not(target_family = \"wasm\"))"),
            normal.target.as_deref()
        );
        assert_eq!(DependencyKind::Normal, normal.kind);
        assert!(normal.has_path);

        let build = find(&edges, "aws-smithy-types");
        assert_eq!(Some("cfg(windows)"), build.target.as_deref());
        assert_eq!(DependencyKind::Build, build.kind);
    }

    /// `aws-smithy-types` declares its optional serde dependency this way.
    #[test]
    fn dotted_target_dependency_entry_is_parsed() {
        let edges = edges(
            r#"
            [target."cfg(aws_sdk_unstable)".dependencies.serde]
            version = "1"
            features = ["derive"]
            optional = true
            "#,
        );
        let edge = find(&edges, "serde");
        assert_eq!(Some("cfg(aws_sdk_unstable)"), edge.target.as_deref());
        assert_eq!(DependencyKind::Normal, edge.kind);
        assert!(edge.optional);
        assert!(!edge.has_path);
    }

    #[test]
    fn dev_dependencies_do_not_propagate_bumps() {
        let edges = edges(
            r#"
            [dev-dependencies]
            aws-smithy-protocol-test = { path = "../aws-smithy-protocol-test", version = "0.63.0" }

            [target.'cfg(windows)'.dev-dependencies]
            aws-smithy-async = { path = "../aws-smithy-async", version = "1.2.0" }
            "#,
        );
        for alias in ["aws-smithy-protocol-test", "aws-smithy-async"] {
            let edge = find(&edges, alias);
            assert_eq!(DependencyKind::Dev, edge.kind);
            assert!(edge.has_path);
            assert!(
                !edge.propagates_bumps(),
                "dev dependency `{alias}` must not propagate bumps"
            );
        }
    }

    #[test]
    fn renamed_dependency_resolves_the_actual_package() {
        let edges = edges(
            r#"
            [dependencies]
            aws-smithy-http-server = { path = "../aws-smithy-http-server", version = "0.67.1" }
            aws-smithy-http-server-065 = { package = "aws-smithy-http-server", version = "0.65" }
            "#,
        );
        let current = find(&edges, "aws-smithy-http-server");
        assert_eq!("aws-smithy-http-server", current.package);
        assert!(current.has_path);

        // The compatibility pin refers to the same package but is version-only, so it must
        // not be treated as following the local crate.
        let pinned = find(&edges, "aws-smithy-http-server-065");
        assert_eq!("aws-smithy-http-server", pinned.package);
        assert!(!pinned.has_path);
    }

    #[test]
    fn version_only_dependencies_have_no_path() {
        let edges = edges(
            r#"
            [dependencies]
            table-form = { version = "0.63.0" }
            string-form = "0.63.0"
            "#,
        );
        for alias in ["table-form", "string-form"] {
            let edge = find(&edges, alias);
            assert!(!edge.has_path, "`{alias}` must not be marked as local");
            assert_eq!(alias, edge.package);
            assert!(edge.unsupported_form.is_none());
        }
    }

    /// Third-party path dependencies are parsed here and filtered out later, when the
    /// package turns out not to be a managed runtime crate.
    #[test]
    fn third_party_path_dependency_is_parsed() {
        let edges = edges(
            r#"
            [dependencies]
            some-vendored-crate = { path = "../../some-vendored-crate", version = "1.0.0" }
            "#,
        );
        let edge = find(&edges, "some-vendored-crate");
        assert!(edge.has_path);
        assert!(edge.unsupported_form.is_none());
    }

    #[test]
    fn unsupported_forms_are_recorded() {
        let edges = edges(
            r#"
            [dependencies]
            inherited = { workspace = true }
            alternate = { version = "1.0.0", registry = "internal" }
            alternate-index = { version = "1.0.0", registry-index = "https://example.com" }
            "#,
        );
        assert_eq!(
            Some("uses workspace dependency inheritance"),
            find(&edges, "inherited").unsupported_form.as_deref()
        );
        assert_eq!(
            Some("specifies an alternate registry"),
            find(&edges, "alternate").unsupported_form.as_deref()
        );
        assert_eq!(
            Some("specifies an alternate registry"),
            find(&edges, "alternate-index").unsupported_form.as_deref()
        );
    }

    #[test]
    fn manifest_without_dependencies_yields_no_edges() {
        assert_eq!(
            Vec::<DependencyEdge>::new(),
            edges(
                r#"
                [package]
                name = "aws-config"
                version = "1.12.0"
                "#
            )
        );
    }

    #[test]
    fn malformed_dependency_entry_fails_with_context() {
        let err = dependency_edges(
            &toml::from_str(
                r#"
                [dependencies]
                broken = 5
                "#,
            )
            .unwrap(),
        )
        .expect_err("5 is not a valid dependency");
        let err = format!("{err:#}");
        assert!(err.contains("`broken`"), "unexpected error: {err}");
        assert!(err.contains("[dependencies]"), "unexpected error: {err}");
    }
}
