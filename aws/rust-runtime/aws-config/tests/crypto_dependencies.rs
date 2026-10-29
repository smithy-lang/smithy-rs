/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![cfg(feature = "credentials-login")]

use std::collections::{HashMap, HashSet};
use std::process::Command;

/// `credentials-login` signs DPoP proofs with `p256`, which reaches `crypto-bigint` through
/// `elliptic-curve` rather than directly, so checking this crate's manifest alone proves nothing.
/// Walk the resolved closure for RUSTSEC-2026-0387.
#[test]
fn credentials_login_resolves_only_patched_bigints() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--all-features",
            "--manifest-path",
        ])
        .arg(&manifest)
        .output()
        .expect("cargo metadata must run; a setup failure is not advisory clearance");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let graph: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let packages = graph["packages"].as_array().unwrap();
    let root = packages
        .iter()
        .find(|package| package["manifest_path"].as_str() == manifest.to_str())
        .unwrap();
    let nodes: HashMap<_, _> = graph["resolve"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| (node["id"].as_str().unwrap(), node))
        .collect();
    let is_normal = |dep: &&serde_json::Value| {
        dep["dep_kinds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|kind| kind["kind"].is_null())
    };
    assert!(
        nodes[root["id"].as_str().unwrap()]["deps"]
            .as_array()
            .unwrap()
            .iter()
            .filter(is_normal)
            .any(|dep| dep["name"] == "p256"),
        "credentials-login must retain its DPoP signing implementation"
    );

    let mut pending = vec![root["id"].as_str().unwrap()];
    let mut reached = HashSet::new();
    while let Some(id) = pending.pop() {
        if reached.insert(id) {
            pending.extend(
                nodes[id]["deps"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(is_normal)
                    .map(|dep| dep["pkg"].as_str().unwrap()),
            );
        }
    }
    let mut affected = Vec::new();
    for package in packages.iter().filter(|package| {
        package["name"] == "crypto-bigint" && reached.contains(package["id"].as_str().unwrap())
    }) {
        let version = package["version"].as_str().unwrap();
        let parts: Vec<_> = version
            .split('.')
            .map(str::parse::<u64>)
            .collect::<Result<_, _>>()
            .expect("review prerelease cryptographic dependencies explicitly");
        assert_eq!(parts.len(), 3);
        if (parts[0], parts[1], parts[2]) < (0, 7, 6) {
            affected.push(version);
        }
    }
    assert!(affected.is_empty(), "RUSTSEC-2026-0387: affected crypto-bigint versions remain in the credentials-login closure: {affected:?}");
}
