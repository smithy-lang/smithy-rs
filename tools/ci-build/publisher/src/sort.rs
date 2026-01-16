/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Logic for topological sorting packages by dependencies.

use anyhow::{anyhow, bail, Result};
use smithy_rs_tool_common::package::{Package, PackageHandle};
use std::collections::{BTreeMap, BTreeSet};

/// Determines the dependency order of the given packages.
pub fn dependency_order(packages: Vec<Package>) -> Result<Vec<Package>> {
    let mut order = Vec::new();
    let mut packages: BTreeMap<PackageHandle, Package> = packages
        .into_iter()
        .map(|p| (p.handle.clone(), p))
        .collect();
    let mut visited = BTreeSet::new();

    let mut to_visit: Vec<&Package> = packages.iter().map(|e| e.1).collect();
    to_visit.sort_by(|a, b| a.local_dependencies.len().cmp(&b.local_dependencies.len()));

    // Depth-first search topological sort
    while let Some(package) = to_visit.iter().find(|e| !visited.contains(&e.handle)) {
        dependency_order_visit(
            &package.handle,
            &packages,
            &mut BTreeSet::new(),
            &mut visited,
            &mut order,
        )?;
    }

    Ok(order
        .into_iter()
        .map(&mut |handle: PackageHandle| packages.remove(&handle).unwrap())
        .collect())
}

fn dependency_order_visit(
    package_handle: &PackageHandle,
    packages: &BTreeMap<PackageHandle, Package>,
    stack: &mut BTreeSet<PackageHandle>,
    visited: &mut BTreeSet<PackageHandle>,
    result: &mut Vec<PackageHandle>,
) -> Result<()> {
    if visited.contains(package_handle) {
        return Ok(());
    }
    if stack.contains(package_handle) {
        tracing::error!(stack = ?stack, handle = ?package_handle, "dependency cycle!");
        bail!("dependency cycle detected");
    }
    stack.insert(package_handle.clone());
    let local_dependencies = &packages
        .get(package_handle)
        .ok_or_else(|| {
            dbg!(packages);
            anyhow!("packages to publish doesn't contain {package_handle:?}")
        })?
        .local_dependencies;
    for dependency in local_dependencies {
        dependency_order_visit(dependency, packages, stack, visited, result)?;
    }
    stack.remove(package_handle);
    visited.insert(package_handle.clone());
    result.push(package_handle.clone());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use semver::Version;
    use smithy_rs_tool_common::package::Publish;

    fn package(name: &str, dependencies: &[&str]) -> Package {
        Package::new(
            PackageHandle::new(name, Version::parse("1.0.0").ok()),
            format!("{name}/Cargo.toml"),
            dependencies
                .iter()
                .map(|d| PackageHandle::new(*d, Version::parse("1.0.0").ok()))
                .collect(),
            Publish::Allowed,
        )
    }

    #[test]
    pub fn test_dependency_order() {
        let packages = vec![
            package("E", &["B", "C", "A"]),
            package("B", &[]),
            package("F", &["E", "D"]),
            package("C", &["A"]),
            package("A", &[]),
            package("D", &["C"]),
        ];

        let result = dependency_order(packages).unwrap();
        assert_eq!(
            "ABCDEF",
            result.iter().fold(String::new(), |mut acc, p| {
                acc.push_str(&p.handle.name);
                acc
            })
        );
    }

    #[test]
    pub fn test_dependency_cycles() {
        let packages = vec![
            package("A", &["C"]),
            package("B", &["A"]),
            package("C", &["B"]),
        ];

        let error = dependency_order(packages).expect_err("cycle");
        assert_eq!("dependency cycle detected", format!("{error}"));
    }

    #[test]
    pub fn complex_tree() {
        let packages = vec![
            package("codeexamples", &["aws-config", "aws-apigateway"]),
            package(
                "aws-apigateway",
                &["aws-config", "aws-types", "aws-endpoint"],
            ),
            package("aws-config", &["aws-endpoint", "aws-types", "aws-sdk-sts"]),
            package("aws-types", &[]),
            package("aws-endpoint", &[]),
            package("aws-sdk-sts", &[]),
        ];
        let result = dependency_order(packages).expect("ok");
        assert_eq!(
            result.iter().map(|p| &p.handle.name).collect::<Vec<_>>(),
            vec![
                "aws-endpoint",
                "aws-sdk-sts",
                "aws-types",
                "aws-config",
                "aws-apigateway",
                "codeexamples"
            ]
        );
    }
}
