/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Reading the graph a leg actually resolved, so a page can name versions instead of symptoms.
//!
//! A page that says `E0308: mismatched types` in `aws_config` tells a responder what the compiler
//! said, not what is wrong. What is wrong is usually a pair of versions: a consumer resolved two
//! incompatible majors of one published crate into the same graph. rustc says so itself, in a note
//! on the rendered diagnostic of a real red leg:
//!
//! ```text
//! note: there are multiple different versions of crate `aws_smithy_schema` in the dependency graph
//!     --> .../aws-smithy-schema-1.2.0/src/schema/protocol.rs:747:1
//!     ::: .../aws-smithy-schema-0.2.1/src/schema/protocol.rs:305:1
//! ```
//!
//! That note is not what this module reads. The same fact is in `Cargo.lock`, which the leg has
//! already written and already uploads as evidence, and taking it from there costs no parsing of
//! compiler prose and does not depend on which error cargo happened to report first.
//!
//! # Why the filter is the whole trick
//!
//! Duplicate versions are ordinary. Measured on two real red legs, 19 of roughly 250 resolved
//! packages were present at more than one version, nearly all of them transitive third-party crates
//! doing nothing wrong — `http`, `hyper`, `rustls`, `syn`. Printing that list on a ticket would bury
//! the answer in noise.
//!
//! Restricted to the crates this canary is about, the same two lockfiles name exactly the culprits:
//!
//! | leg | duplicated, filtered | what the compiler said |
//! |---|---|---|
//! | `prior-aws-config-1-10--stable` | `aws-smithy-json 0.63.1, 0.65.1`; `aws-smithy-schema 0.2.1, 1.2.0` | blamed `aws_smithy_schema` by name |
//! | `ssstar--stable` | `aws-smithy-http 0.60.12, 0.64.3` | `unresolved import aws_smithy_http::event_stream` |
//!
//! # This is evidence, never a verdict
//!
//! Two majors of one crate in one graph is legal, intended, and usually harmless: Cargo permits it
//! so that a graph can migrate one dependency at a time. It breaks only when a type crosses the
//! boundary, which is why the first leg above lists a duplicated `aws-smithy-json` that had nothing
//! to do with the failure. So this module annotates a failure already established by other means,
//! and never establishes one — [`crate::classify`] does not consult it, and must not.

use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;

use crate::publish::{AWS_SDK_RUNTIME, ENTIRE_SMITHY_RUNTIME};

/// One crate under test that resolved to more than one version in the same graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Duplicated {
    pub name: String,
    /// Every version present, in ascending order.
    pub versions: Vec<String>,
}

impl fmt::Display for Duplicated {
    /// `aws-smithy-schema 0.2.1 and 1.2.0`, which is how it should read on a ticket.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.name, join_and(&self.versions))
    }
}

/// `a`, `a and b`, `a, b and c`.
fn join_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The packages a `Cargo.lock` resolved, keyed by name because the interesting question is which
/// names have more than one version.
#[derive(Debug, Clone, Default)]
pub struct Resolved(BTreeMap<String, Vec<String>>);

/// Just enough of `Cargo.lock` to answer "what resolved, at what version". Everything else in the
/// file — `source`, `checksum`, `dependencies` — is ignored, so this keeps working across lockfile
/// format versions, which is worth having for a file the canary does not control.
#[derive(serde::Deserialize)]
struct Lock {
    #[serde(default)]
    package: Vec<Package>,
}

#[derive(serde::Deserialize)]
struct Package {
    name: String,
    version: String,
}

impl Resolved {
    pub fn parse(lock: &str) -> anyhow::Result<Self> {
        let lock: Lock = toml::from_str(lock)?;
        let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for p in lock.package {
            map.entry(p.name).or_default().push(p.version);
        }
        for versions in map.values_mut() {
            versions.sort_by_key(|v| version_key(v));
            versions.dedup();
        }
        Ok(Self(map))
    }

    /// How many packages resolved, counting a duplicated name once per version.
    pub fn len(&self) -> usize {
        self.0.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The versions a package resolved to. Empty if it is not in the graph.
    pub fn versions_of(&self, name: &str) -> &[String] {
        self.0.get(name).map(Vec::as_slice).unwrap_or_default()
    }

    /// Every resolved package and its versions, in name order.
    pub fn packages(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.0.iter().map(|(n, v)| (n.as_str(), v.as_slice()))
    }

    /// The version of the package a *cargo target* belongs to, rendered for a ticket.
    ///
    /// Cargo reports a lib target as `aws_config` where the package is `aws-config`, so the
    /// underscore form is tried first — a package really named with underscores would otherwise be
    /// missed — and the hyphenated form second.
    pub fn version_of_target(&self, target: &str) -> Option<String> {
        let versions = match self.versions_of(target) {
            [] => self.versions_of(&target.replace('_', "-")),
            found => found,
        };
        match versions {
            [] => None,
            found => Some(join_and(found)),
        }
    }

    /// Crates under test present at more than one version: the version pair a page needs.
    ///
    /// "Under test" is both published lists — the smithy-rs runtime crates this repository releases
    /// and the six SDK runtime crates. See the module docs for why the filter is what makes the
    /// result useful, and why a hit here is evidence rather than a fault.
    pub fn duplicated_crates_under_test(&self) -> Vec<Duplicated> {
        self.0
            .iter()
            .filter(|(_, versions)| versions.len() > 1)
            .filter(|(name, _)| is_under_test(name))
            .map(|(name, versions)| Duplicated {
                name: name.clone(),
                versions: versions.clone(),
            })
            .collect()
    }
}

/// Whether a resolved package is one this canary is watching, as opposed to a third-party
/// dependency that happens to be in the graph.
fn is_under_test(name: &str) -> bool {
    ENTIRE_SMITHY_RUNTIME.contains(&name) || AWS_SDK_RUNTIME.contains(&name)
}

/// Order versions by their numeric components rather than as text.
///
/// Text order puts `1.9.10` before `1.9.4`, which on a ticket looks like a mistake and invites the
/// reader to distrust the rest of the line. Not a semver implementation and not trying to be: the
/// remainder is compared as text, so `1.0.0-alpha.2` and `1.0.0-alpha.10` may still be shown in the
/// wrong order. That is a presentation detail of a set whose membership is what matters.
fn version_key(v: &str) -> (u64, u64, u64, String) {
    let mut parts = v.splitn(3, '.');
    let num = |p: Option<&str>| -> u64 {
        p.unwrap_or_default()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .unwrap_or(0)
    };
    let major = num(parts.next());
    let minor = num(parts.next());
    let tail = parts.next().unwrap_or_default();
    let patch = num(Some(tail));
    let rest = tail
        .trim_start_matches(|c: char| c.is_ascii_digit())
        .to_string();
    (major, minor, patch, rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from the real `Cargo.lock` of `prior-aws-config-1-10--stable`, the leg whose
    /// `E0308` rustc blamed on two versions of `aws_smithy_schema`. The third-party duplicates are
    /// kept because they are the noise the filter has to remove.
    const PRIOR_AWS_CONFIG_LOCK: &str = r#"
version = 4

[[package]]
name = "canary-build-check"
version = "0.1.0"
dependencies = ["aws-config"]

[[package]]
name = "aws-config"
version = "1.10.1"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "deadbeef"

[[package]]
name = "aws-smithy-schema"
version = "1.2.0"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "aws-smithy-schema"
version = "0.2.1"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "aws-smithy-json"
version = "0.65.1"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "aws-smithy-json"
version = "0.63.1"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "aws-types"
version = "1.7.0"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "http"
version = "1.5.0"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "http"
version = "0.2.12"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "rustls"
version = "0.23.46"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "rustls"
version = "0.21.12"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;

    #[test]
    fn it_reads_names_and_versions() {
        let r = Resolved::parse(PRIOR_AWS_CONFIG_LOCK).unwrap();
        assert_eq!(r.len(), 11);
        assert_eq!(r.versions_of("aws-config"), ["1.10.1"]);
        assert_eq!(r.versions_of("aws-smithy-schema"), ["0.2.1", "1.2.0"]);
        assert!(
            r.versions_of("serde").is_empty(),
            "a crate not in the graph has no versions, rather than being an error"
        );
    }

    /// The point of the module. Nineteen of the real lockfile's packages are duplicated; the filter
    /// has to leave only the ones a responder should look at.
    #[test]
    fn it_reports_only_duplicated_crates_under_test() {
        let r = Resolved::parse(PRIOR_AWS_CONFIG_LOCK).unwrap();
        let dups: Vec<String> = r
            .duplicated_crates_under_test()
            .iter()
            .map(Duplicated::to_string)
            .collect();
        assert_eq!(
            dups,
            [
                "aws-smithy-json 0.63.1 and 0.65.1",
                "aws-smithy-schema 0.2.1 and 1.2.0",
            ],
            "http and rustls are duplicated too, and are not ours"
        );
    }

    /// `aws-types` is in `AWS_SDK_RUNTIME`, not in `ENTIRE_SMITHY_RUNTIME`, and is excluded from
    /// the publish wait on purpose. It must still be attributable when it is the thing duplicated.
    #[test]
    fn an_sdk_runtime_crate_counts_as_under_test() {
        let lock = r#"
version = 4

[[package]]
name = "aws-types"
version = "1.7.0"

[[package]]
name = "aws-types"
version = "0.56.1"
"#;
        let r = Resolved::parse(lock).unwrap();
        assert_eq!(
            r.duplicated_crates_under_test()
                .iter()
                .map(Duplicated::to_string)
                .collect::<Vec<_>>(),
            ["aws-types 0.56.1 and 1.7.0"]
        );
    }

    /// A graph with no duplicate of anything ours is the healthy case, and must produce nothing
    /// rather than a list of third-party duplicates.
    #[test]
    fn a_healthy_graph_reports_no_duplicates() {
        let lock = r#"
version = 4

[[package]]
name = "aws-smithy-types"
version = "1.9.4"

[[package]]
name = "http"
version = "1.5.0"

[[package]]
name = "http"
version = "0.2.12"
"#;
        let r = Resolved::parse(lock).unwrap();
        assert!(r.duplicated_crates_under_test().is_empty());
    }

    /// Cargo names a lib target with underscores, so the mapping back to a package is the whole
    /// reason this lookup exists.
    #[test]
    fn a_cargo_target_maps_back_to_its_package_version() {
        let r = Resolved::parse(PRIOR_AWS_CONFIG_LOCK).unwrap();
        assert_eq!(
            r.version_of_target("aws_config").as_deref(),
            Some("1.10.1"),
            "the target cargo reported on the real red leg"
        );
        assert_eq!(
            r.version_of_target("aws-config").as_deref(),
            Some("1.10.1"),
            "and the package name itself still works"
        );
        assert_eq!(
            r.version_of_target("aws_smithy_schema").as_deref(),
            Some("0.2.1 and 1.2.0"),
            "a duplicated package must not be reported as one arbitrary version"
        );
        assert_eq!(r.version_of_target("ssstar"), None);
    }

    /// A package genuinely named with underscores must not be mangled into a miss.
    #[test]
    fn an_underscored_package_name_is_tried_before_hyphenating() {
        let lock = r#"
version = 4

[[package]]
name = "nix_sys"
version = "2.0.0"
"#;
        let r = Resolved::parse(lock).unwrap();
        assert_eq!(r.version_of_target("nix_sys").as_deref(), Some("2.0.0"));
    }

    #[test]
    fn versions_sort_numerically_not_as_text() {
        let lock = r#"
version = 4

[[package]]
name = "aws-smithy-types"
version = "1.9.10"

[[package]]
name = "aws-smithy-types"
version = "1.9.4"
"#;
        let r = Resolved::parse(lock).unwrap();
        assert_eq!(
            r.versions_of("aws-smithy-types"),
            ["1.9.4", "1.9.10"],
            "text order would put 1.9.10 first and look like a bug"
        );
    }

    #[test]
    fn a_prerelease_version_does_not_panic_or_sort_before_its_release() {
        let lock = r#"
version = 4

[[package]]
name = "aws-smithy-types"
version = "1.0.0-alpha.1"

[[package]]
name = "aws-smithy-types"
version = "1.0.0"
"#;
        let r = Resolved::parse(lock).unwrap();
        // "" sorts before "-alpha.1", so the release leads. Semver says the opposite; this is a
        // display order for a two-element set, not a precedence decision.
        assert_eq!(
            r.versions_of("aws-smithy-types"),
            ["1.0.0", "1.0.0-alpha.1"]
        );
    }

    #[test]
    fn an_unparseable_lockfile_is_an_error_not_a_panic() {
        assert!(Resolved::parse("[[package]]\nname = ").is_err());
        assert!(
            Resolved::parse("").unwrap().is_empty(),
            "an empty file is empty, not malformed"
        );
    }

    #[test]
    fn join_and_reads_as_prose() {
        let s = |v: &[&str]| join_and(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(s(&[]), "");
        assert_eq!(s(&["1.0.0"]), "1.0.0");
        assert_eq!(s(&["1.0.0", "2.0.0"]), "1.0.0 and 2.0.0");
        assert_eq!(s(&["1.0.0", "2.0.0", "3.0.0"]), "1.0.0, 2.0.0 and 3.0.0");
    }

    /// The real file, not a fixture: the lockfile shapes this has to survive are cargo's, and the
    /// repository has plenty of them. Guards against a format change making every diagnosis silent.
    #[test]
    fn it_parses_a_real_lockfile_from_the_repository() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repo root");
        let path = root.join("aws/rust-runtime/Cargo.lock");
        let text = std::fs::read_to_string(&path).expect("the SDK runtime lockfile");
        let r = Resolved::parse(&text).unwrap();
        assert!(r.len() > 100, "{} packages is too few", r.len());
        assert!(
            !r.versions_of("aws-smithy-types").is_empty(),
            "a lockfile for the runtime must resolve aws-smithy-types"
        );
    }
}
