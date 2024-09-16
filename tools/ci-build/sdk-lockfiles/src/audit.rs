/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::AuditArgs;
use anyhow::bail;
use anyhow::{Context, Result};
use cargo_lock::dependency::graph::{Graph, NodeIndex};
use cargo_lock::{package::Package, Lockfile};
use petgraph::visit::EdgeRef;
use smithy_rs_tool_common::git::find_git_repository_root;
use smithy_rs_tool_common::here;
use std::collections::{BTreeSet, HashSet};
use std::env;
use std::iter;
use std::path::PathBuf;

// A list of AWS runtime crate must be in sync with
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

// A list of server runtime crates must be in sync with
// https://github.com/smithy-lang/smithy-rs/blob/0f9b9aba386ea3063912a0464ba6a1fd7c596018/buildSrc/src/main/kotlin/CrateSet.kt#L85-L87
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

// Recursively traverses a chain of dependencies originating from a potential new dependency. Returns true as soon as
// it encounters a crate name that matches a runtime crate used by the AWS SDK.
fn visit(graph: &Graph, node_index: NodeIndex, visited: &mut BTreeSet<NodeIndex>) -> bool {
    if !visited.insert(node_index) {
        return false;
    }

    let dependencies = graph
        .edges_directed(
            node_index,
            cargo_lock::dependency::graph::EdgeDirection::Incoming,
        )
        .map(|edge| edge.source())
        .collect::<Vec<_>>();

    for dependency_node_index in dependencies.iter() {
        let package = &graph[*dependency_node_index];
        tracing::debug!("visiting `{}`", package.name.as_str());
        if new_dependency_for_aws_sdk(package.name.as_str()) {
            tracing::debug!("it's a new dependency for the AWS SDK!");
            return true;
        }
        if visit(graph, *dependency_node_index, visited) {
            return true;
        }
    }

    false
}

// Checks if the `target` dependency is introduced by a runtime crate used by the AWS SDK.
//
// This function considers `target` a new dependency if it is used by a runtime crate, whether directly or indirectly,
// that is part of the AWS SDK.
fn new_dependency(lockfile: &Lockfile, target: &str) -> bool {
    tracing::debug!(
            "`{}` is not recorded in the SDK lockfile. Verifying whether it is a new dependency for the AWS SDK...",
            target
    );
    let tree = lockfile.dependency_tree().unwrap();
    let indices: Vec<_> = [target.to_owned()]
        .iter()
        .map(|dep| {
            let package = lockfile
                .packages
                .iter()
                .find(|pkg| pkg.name.as_str() == dep)
                .unwrap();
            tree.nodes()[&package.into()]
        })
        .collect();

    for index in &indices {
        let mut visited: BTreeSet<NodeIndex> = BTreeSet::new();
        tracing::debug!("traversing a dependency chain for `{}`...", target);
        if visit(tree.graph(), *index, &mut visited) {
            return true;
        }
    }

    tracing::debug!("`{}` is not a new dependency for the AWS SDK", target);
    false
}

// Verifies if all dependencies listed in `runtime_lockfile` are present in `sdk_dependency_set`, and returns an
// iterator that yields those not found in the set.
//
// This check is based solely on crate names, ignoring other metadata such as versions or sources.
fn audit_runtime_lockfile_covered_by_sdk_lockfile<'a>(
    runtime_lockfile: &'a Lockfile,
    sdk_dependency_set: &'a HashSet<&str>,
) -> impl Iterator<Item = &'a Package> + 'a {
    runtime_lockfile.packages.iter().filter(move |p| {
        !sdk_dependency_set.contains(p.name.as_str())
            && new_dependency(runtime_lockfile, p.name.as_str())
    })
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

    let mut uncovered = Vec::new();

    for (runtime_lockfile, path) in &runtime_lockfiles {
        tracing::info!(
            "checking whether `{}` is covered by the SDK lockfile...",
            path
        );
        uncovered.extend(
            audit_runtime_lockfile_covered_by_sdk_lockfile(runtime_lockfile, &sdk_dependency_set)
                .zip(iter::repeat(path)),
        );
    }

    if uncovered.is_empty() {
        println!("SUCCESS");
        Ok(())
    } else {
        for (pkg, origin_lockfile) in uncovered {
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
        )
        .next()
        .is_none());
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
        )
        .next()
        .is_none());
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
                )
                .map(|p| p.name.as_str())
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
                )
                .map(|p| p.name.as_str())
                .sorted()
                .collect::<Vec<_>>(),
            );
        }
    }
}
