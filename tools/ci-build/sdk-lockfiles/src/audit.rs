/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::AuditArgs;
use anyhow::bail;
use anyhow::{Context, Result};
use cargo_lock::dependency::graph::{Graph, NodeIndex};
use cargo_lock::{package::Package, Lockfile, Name, Version};
use petgraph::visit::EdgeRef;
use smithy_rs_tool_common::git::find_git_repository_root;
use smithy_rs_tool_common::here;
use std::collections::HashSet;
use std::env;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::iter;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::LazyLock;

// Struct representing a potential dependency that may eventually be reported as an error by `audit`,
// indicating that the crate `to` is not covered by the SDK lockfile even though `to` is reported as a dependency
// of the crate `from` in a runtime lockfile.
//
// This dependency might be an indirect dependency, meaning that the crate `from` transitively depends on the crate `to`.
// Given collected `SuspectDependency` instances, the `audit` subcommand refers to `FALSE_POSITIVES` to determine
// whether a `SuspectDependency` should be reported as an audit error.
struct SuspectDependency {
    from: Package,
    to: Package,
}

impl fmt::Debug for SuspectDependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`SuspectDependency`: {} -> {}",
            self.from.name.as_str(),
            self.to.name.as_str()
        )
    }
}

impl PartialEq for SuspectDependency {
    fn eq(&self, other: &Self) -> bool {
        // `true` if two `SuspectDependency` share the same names `from`s and `to`s, ignoring package versions.
        self.from.name == other.from.name || self.to.name == other.to.name
    }
}

impl Eq for SuspectDependency {}

impl Hash for SuspectDependency {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.from.name.as_str().hash(state);
        self.to.name.as_str().hash(state);
    }
}

impl SuspectDependency {
    fn new(from: Package, to: Package) -> Self {
        Self { from, to }
    }
}

// Creates a `Package` where only the package name matters, while the other the fields do not matter
//
// An example of this is the equality of two `SuspectDependency` instances that relies solely on package names.
fn package_with_name(name: &str) -> Package {
    Package {
        name: Name::from_str(name).expect("valid package name"),
        version: Version::new(0, 0, 1),
        source: None,
        checksum: None,
        dependencies: vec![],
        replace: None,
    }
}

// The tool has a limitation where `audit` may report false positives based on the contents of a lockfile.
// For example, if a section of the file appears as follows:
//
//   pin-project v1.1.5
//   ├── tower v0.4.13
//   │   ├── aws-smithy-experimental v0.1.4
//   │   ├── aws-smithy-http-server v0.63.3
//   │   │   └── aws-smithy-http-server-python v0.63.2
//   │   ├── aws-smithy-http-server-python v0.63.2
//   ...
//
// The tool cannot identify which dependent crate of `tower` enables `tower`'s Cargo feature to include `pin-project`.
// In the case above, `aws-smithy-experimental` does not enable this feature, while `aws-smithy-http-server` does.
// When `aws-smithy-experimental` is compiled alone for a generated SDK without server-related Smithy runtime crates,
// `pin-project` will not appear in the SDK lockfile. Therefore, the claim that `aws-smithy-experimental` depends
// on `pin-project` in the above section is incorrect, and we need to teach the tool to recognize this.
//
// The following set serves as an allowlist whose entries should not be reported as audit errors.
static FALSE_POSITIVES: LazyLock<HashSet<SuspectDependency>> = LazyLock::new(|| {
    include_str!("../false-positives.txt")
        .lines()
        .map(|line| {
            let parts: Vec<&str> = line.split("->").map(|s| s.trim()).collect();
            SuspectDependency::new(package_with_name(parts[0]), package_with_name(parts[1]))
        })
        .collect()
});

// A list of the names of AWS runtime crates (crate versions do not need to match) must be in sync with
// https://github.com/smithy-lang/smithy-rs/blob/0f9b9aba386ea3063912a0464ba6a1fd7c596018/buildSrc/src/main/kotlin/CrateSet.kt#L42-L53
const AWS_SDK_RUNTIMES: &[&str] = &[
    "aws-config",
    "aws-credential-types",
    "aws-endpoint",
    "aws-http",
    "aws-hyper",
    "aws-runtime",
    "aws-runtime-api",
    "aws-sig-auth",
    "aws-sigv4",
    "aws-types",
];

// A list of the names of server specific runtime crates (crate versions do not need to match) must be in sync with
// https://github.com/smithy-lang/smithy-rs/blob/main/buildSrc/src/main/kotlin/CrateSet.kt#L42
const SERVER_SPECIFIC_RUNTIMES: &[&str] = &[
    "aws-smithy-http-server",
    "aws-smithy-http-server-python",
    "aws-smithy-http-typescript",
];

fn new_dependency_for_aws_sdk(crate_name: &str) -> bool {
    AWS_SDK_RUNTIMES.contains(&crate_name)
        || (crate_name.starts_with("aws-smithy-")
            && !SERVER_SPECIFIC_RUNTIMES.contains(&crate_name))
}

// Recursively traverses a chain of dependencies originating from a potential new dependency, populating
// `suspect_dependencies` as it encounters a runtime crates used by the AWS SDK that appear to consume
// `target_package`.
fn is_consumed_by_aws_sdk(
    graph: &Graph,
    target_package: &Package,
    node_index: NodeIndex,
    suspect_dependencies: &mut HashSet<SuspectDependency>,
    visited: &mut HashSet<NodeIndex>,
) {
    if !visited.insert(node_index) {
        return;
    }

    let consumers = graph
        .edges_directed(
            node_index,
            cargo_lock::dependency::graph::EdgeDirection::Incoming,
        )
        .map(|edge| edge.source())
        .collect::<Vec<_>>();

    for consumer_node_index in consumers.iter() {
        let package = &graph[*consumer_node_index];
        tracing::debug!("visiting `{}`", package.name.as_str());
        if new_dependency_for_aws_sdk(package.name.as_str()) {
            suspect_dependencies.insert(SuspectDependency::new(
                package.clone(),
                target_package.clone(),
            ));
        }
        is_consumed_by_aws_sdk(
            graph,
            target_package,
            *consumer_node_index,
            suspect_dependencies,
            visited,
        )
    }
}

// Collects a set of `SuspectDependency` instances as it encounters cases where `target_package` appearing in `lockfile`
// is introduced by a runtime crate used by the AWS SDK, whether directly or indirectly
fn collect_suspect_dependencies(
    lockfile: &Lockfile,
    target_package: &Package,
) -> HashSet<SuspectDependency> {
    let target_package_name = target_package.name.as_str();
    tracing::debug!(
            "`{}` is not recorded in the SDK lockfile. Verifying whether it is a new dependency for the AWS SDK...",
            target_package_name
    );
    let tree = lockfile.dependency_tree().unwrap();
    let package = lockfile
        .packages
        .iter()
        .find(|pkg| pkg.name.as_str() == target_package_name)
        .expect("{target_package_name} must be in dependencies listed in `lockfile`");
    let indices = vec![tree.nodes()[&package.into()]];

    let mut suspect_dependencies = HashSet::new();

    for index in &indices {
        let mut visited: HashSet<NodeIndex> = HashSet::new();
        tracing::debug!(
            "traversing a dependency chain for `{}`...",
            target_package_name
        );
        is_consumed_by_aws_sdk(
            tree.graph(),
            target_package,
            *index,
            &mut suspect_dependencies,
            &mut visited,
        );
    }

    suspect_dependencies
}

// Verifies if all dependencies listed in `runtime_lockfile` are present in `sdk_dependency_set` and returns a set of
// `SuspectDependency` instances after consulting the set against the provided `false_positives`
//
// Each entry in this set indicates that a crate represented by `SuspectDependency::to` will be reported as an audit
// error, saying it appears in `runtime_lockfile` but is not covered by the SDK lockfile
//
// This check is based solely on crate names, ignoring other metadata such as versions or sources.
fn audit_runtime_lockfile_covered_by_sdk_lockfile<'a>(
    runtime_lockfile: &'a Lockfile,
    sdk_dependency_set: &'a HashSet<&str>,
    false_positives: Option<&HashSet<SuspectDependency>>,
) -> HashSet<SuspectDependency> {
    let mut suspect_dependencies = HashSet::new();
    for package in &runtime_lockfile.packages {
        if !sdk_dependency_set.contains(package.name.as_str()) {
            suspect_dependencies
                .extend(collect_suspect_dependencies(runtime_lockfile, package).into_iter());
        }
    }
    if let Some(false_positives) = false_positives {
        // Any entry in `false_positives` that is not reported in `suspect_dependencies` may be removed,
        // as it is no longer considered a false positive.
        for fp in false_positives.difference(&suspect_dependencies) {
            tracing::warn!("{fp:?} may potentially be removed from `{false_positives:?}`");
        }
        suspect_dependencies.retain(|dep| !false_positives.contains(dep));
        suspect_dependencies
    } else {
        suspect_dependencies
    }
}

fn lockfile_for(
    smithy_rs_root: PathBuf,
    relative_path_to_lockfile: &str,
) -> Result<(Lockfile, &str)> {
    let mut lockfile = smithy_rs_root;
    lockfile.push(relative_path_to_lockfile);
    Ok((
        Lockfile::load(lockfile).with_context(|| {
            format!(
                "failed to crate a `Lockfile` for {}",
                relative_path_to_lockfile
            )
        })?,
        relative_path_to_lockfile,
    ))
}

pub(super) fn audit(args: AuditArgs) -> Result<()> {
    let cwd = if let Some(smithy_rs_path) = args.smithy_rs_path {
        smithy_rs_path
    } else {
        env::current_dir().context("failed to get current working directory")?
    };
    let smithy_rs_root = find_git_repository_root("smithy-rs", cwd).context(here!())?;

    let (sdk_lockfile, _) = lockfile_for(smithy_rs_root.clone(), "aws/sdk/Cargo.lock")?;
    let sdk_dependency_set = sdk_lockfile
        .packages
        .iter()
        .map(|p| p.name.as_str())
        .collect::<HashSet<_>>();

    let runtime_lockfiles = [
        lockfile_for(smithy_rs_root.clone(), "rust-runtime/Cargo.lock")?,
        lockfile_for(smithy_rs_root.clone(), "aws/rust-runtime/Cargo.lock")?,
        lockfile_for(smithy_rs_root, "aws/rust-runtime/aws-config/Cargo.lock")?,
    ];

    let mut crates_to_report: Vec<(Package, &str)> = Vec::new();

    for (runtime_lockfile, path) in &runtime_lockfiles {
        tracing::info!(
            "checking whether `{}` is covered by the SDK lockfile...",
            path
        );

        let crates_uncovered_by_sdk = audit_runtime_lockfile_covered_by_sdk_lockfile(
            runtime_lockfile,
            &sdk_dependency_set,
            Some(&FALSE_POSITIVES),
        );

        crates_to_report.extend(
            crates_uncovered_by_sdk
                .into_iter()
                .map(|c| c.to)
                .zip(iter::repeat(*path)),
        );
    }

    if crates_to_report.is_empty() {
        println!("SUCCESS");
        Ok(())
    } else {
        for (pkg, origin_lockfile) in crates_to_report {
            eprintln!(
                "`{}` ({}), used by `{}`, is not contained in the SDK lockfile!",
                pkg.name.as_str(),
                pkg.version,
                origin_lockfile,
            );
        }
        bail!("there are lockfile audit failures")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use itertools::Itertools;
    use std::str::FromStr;
    use tracing_test::traced_test;

    // For simplicity, return an SDK dependency set with a small subset of crates. If a runtime crate used by
    // subsequent tests is omitted, it will not affect the functionality of the system under test,
    // `audit_runtime_lockfile_covered_by_sdk_lockfile`.
    fn sdk_dependency_set() -> HashSet<&'static str> {
        let mut result = HashSet::new();
        result.insert("aws-credential-types");
        result.insert("aws-sigv4");
        result.insert("aws-smithy-cbor");
        result.insert("aws-smithy-runtime");
        result.insert("fastrand");
        result.insert("zeroize");
        result
    }

    #[test]
    fn dependency_is_covered_by_sdk() {
        let runtime_lockfile = Lockfile::from_str(
            r#"
[[package]]
name = "aws-credential-types"
version = "1.2.1"
dependencies = [
 "zeroize",
]

[[package]]
name = "aws-smithy-runtime"
version = "1.7.1"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "d1ce695746394772e7000b39fe073095db6d45a862d0767dd5ad0ac0d7f8eb87"
dependencies = [
 "fastrand",
]

[[package]]
name = "fastrand"
version = "2.0.2"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "658bd65b1cf4c852a3cc96f18a8ce7b5640f6b703f905c7d74532294c2a63984"

[[package]]
name = "zeroize"
version = "1.8.1"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "ced3678a2879b30306d323f4542626697a464a97c0a07c9aebf7ebca65cd4dde"
"#,
        )
        .unwrap();

        assert!(audit_runtime_lockfile_covered_by_sdk_lockfile(
            &runtime_lockfile,
            &sdk_dependency_set(),
            None,
        )
        .is_empty());
    }

    #[test]
    fn new_dependency_but_introduced_by_crate_irrelevant_to_sdk() {
        let runtime_lockfile = Lockfile::from_str(
            r#"
[[package]]
name = "aws-smithy-http-server-python"
version = "0.63.2"
dependencies = [
 "pyo3-asyncio",
]

[[package]]
name = "pyo3-asyncio"
version = "0.18.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "d3564762e37035cfc486228e10b0528460fa026d681b5763873c693aa0d5c260"
dependencies = [
 "inventory",
]

[[package]]
name = "inventory"
version = "0.3.15"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "f958d3d68f4167080a18141e10381e7634563984a537f2a49a30fd8e53ac5767"
"#,
        )
        .unwrap();

        assert!(audit_runtime_lockfile_covered_by_sdk_lockfile(
            &runtime_lockfile,
            &sdk_dependency_set(),
            None,
        )
        .is_empty());
    }

    #[test]
    fn new_dependency_for_sdk() {
        // New dependencies originating from the smithy runtime crates
        {
            let runtime_lockfile = Lockfile::from_str(
                r#"
[[package]]
name = "aws-smithy-cbor"
version = "0.60.7"
dependencies = [
 "minicbor",
]

[[package]]
name = "aws-smithy-compression"
version = "0.0.1"
dependencies = [
 "flate2"
]

[[package]]
name = "flate2"
version = "1.0.33"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "324a1be68054ef05ad64b861cc9eaf1d623d2d8cb25b4bf2cb9cdd902b4bf253"

[[package]]
name = "minicbor"
version = "0.24.2"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "5f8e213c36148d828083ae01948eed271d03f95f7e72571fa242d78184029af2"
"#,
            )
            .unwrap();

            assert_eq!(
                vec!["flate2", "minicbor"],
                audit_runtime_lockfile_covered_by_sdk_lockfile(
                    &runtime_lockfile,
                    &sdk_dependency_set(),
                    None,
                )
                .into_iter()
                .map(|suspect_dependency| suspect_dependency.to.name.as_str().to_owned())
                .sorted()
                .collect::<Vec<_>>(),
            );
        }

        // New dependencies originating from the AWS runtime crates
        {
            let runtime_lockfile = Lockfile::from_str(
                r#"
[[package]]
name = "aws-credential-types"
version = "1.2.1"
dependencies = [
 "zeroize",
]

[[package]]
name = "aws-sigv4"
version = "1.2.3"
dependencies = [
 "ahash",
 "aws-credential-types",
 "lru",
 "p256",
]

[[package]]
name = "ahash"
version = "0.8.11"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "e89da841a80418a9b391ebaea17f5c112ffaaa96f621d2c285b5174da76b9011"
dependencies = [
 "zerocopy",
]

[[package]]
name = "hashbrown"
version = "0.14.5"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "e5274423e17b7c9fc20b6e7e208532f9b19825d82dfd615708b70edd83df41f1"
dependencies = [
 "ahash",
]

[[package]]
name = "lru"
version = "0.12.4"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "37ee39891760e7d94734f6f63fedc29a2e4a152f836120753a72503f09fcf904"
dependencies = [
 "hashbrown",
]

[[package]]
name = "p256"
version = "0.11.1"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "51f44edd08f51e2ade572f141051021c5af22677e42b7dd28a88155151c33594"

[[package]]
name = "zerocopy"
version = "0.7.35"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "1b9b4fd18abc82b8136838da5d50bae7bdea537c574d8dc1a34ed098d6c166f0"

[[package]]
name = "zeroize"
version = "1.8.1"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "ced3678a2879b30306d323f4542626697a464a97c0a07c9aebf7ebca65cd4dde"
"#,
            )
            .unwrap();

            assert_eq!(
                vec!["ahash", "hashbrown", "lru", "p256", "zerocopy"],
                audit_runtime_lockfile_covered_by_sdk_lockfile(
                    &runtime_lockfile,
                    &sdk_dependency_set(),
                    None,
                )
                .into_iter()
                .map(|suspect_dependency| suspect_dependency.to.name.as_str().to_owned())
                .sorted()
                .collect::<Vec<_>>(),
            );
        }
    }

    #[test]
    fn test_false_positives() {
        let mut sdk_dependency_set = HashSet::new();
        sdk_dependency_set.insert("aws-smithy-experimental");
        sdk_dependency_set.insert("tower");

        let runtime_lockfile = Lockfile::from_str(
            r#"
[[package]]
name = "aws-smithy-experimental"
version = "0.1.4"
dependencies = [
 "tower",
]

[[package]]
name = "aws-smithy-http-server"
version = "0.63.2"
dependencies = [
 "tower",
]

[[package]]
name = "tower"
version = "0.4.13"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "b8fa9be0de6cf49e536ce1851f987bd21a43b771b09473c3549a6c853db37c1c"
dependencies = [
 "pin-project",
]

[[package]]
name = "pin-project"
version = "1.1.5"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "b6bf43b791c5b9e34c3d182969b4abb522f9343702850a2e57f460d00d09b4b3"
"#,
        )
        .unwrap();

        let mut false_positives = HashSet::new();
        false_positives.insert(SuspectDependency::new(
            package_with_name("aws-smithy-experimental"),
            package_with_name("pin-project"),
        ));

        assert!(audit_runtime_lockfile_covered_by_sdk_lockfile(
            &runtime_lockfile,
            &sdk_dependency_set,
            Some(&false_positives),
        )
        .is_empty());
    }

    #[test]
    #[traced_test]
    fn test_warning_issued_when_false_positive_is_no_longer_false_positive() {
        let mut sdk_dependency_set = HashSet::new();
        sdk_dependency_set.insert("aws-smithy-experimental");
        sdk_dependency_set.insert("tower");
        sdk_dependency_set.insert("pin-project"); // include `pin-project` in the SDK lockfile

        let runtime_lockfile = Lockfile::from_str(
            r#"
[[package]]
name = "aws-smithy-experimental"
version = "0.1.4"
dependencies = [
 "tower",
]

[[package]]
name = "aws-smithy-http-server"
version = "0.63.2"
dependencies = [
 "tower",
]

[[package]]
name = "tower"
version = "0.4.13"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "b8fa9be0de6cf49e536ce1851f987bd21a43b771b09473c3549a6c853db37c1c"
dependencies = [
 "pin-project",
]

[[package]]
name = "pin-project"
version = "1.1.5"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "b6bf43b791c5b9e34c3d182969b4abb522f9343702850a2e57f460d00d09b4b3"
"#,
        )
        .unwrap();

        let mut false_positives = HashSet::new();
        false_positives.insert(SuspectDependency::new(
            package_with_name("aws-smithy-experimental"),
            package_with_name("pin-project"),
        ));

        assert!(audit_runtime_lockfile_covered_by_sdk_lockfile(
            &runtime_lockfile,
            &sdk_dependency_set,
            Some(&false_positives),
        )
        .is_empty());

        assert!(logs_contain("may potentially be removed from"));
    }
}
