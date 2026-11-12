/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Knowing when a release has finished publishing.
//!
//! The canary's first tier waits for the release to land on crates.io before handing over to the
//! 24-hour floor tier. Getting that end condition wrong is expensive in both directions: finish
//! early and the legs test a half-published graph, reporting breaks that are really just crates
//! that have not arrived yet; finish late or never and the floor tier never starts.
//!
//! # A checklist, not a quiet period
//!
//! The release is finished when crates.io has every version this tag declares, and until then the
//! outstanding set names exactly what is missing. No tunable threshold, so no window that can open
//! on a partial graph without saying so.
//!
//! # What counts as "arrived"
//!
//! Exact membership: crates.io lists this precise version string. Not "crates.io's newest is at
//! least ours", which needs semver comparison and gets the September 2026 shape wrong — that
//! release republished `aws-smithy-types` *downwards*, from a yanked 1.7.0 to 1.6.4, so a
//! greater-or-equal test would have reported it arrived before it had.
//!
//! Membership also makes the predicate idempotent, which collapses two concepts into one: the set
//! that is "expected" and the set that is "still outstanding" are the same computation, so a
//! watcher that starts late is automatically correct about crates that already landed.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

/// Every crate a release of this repository publishes from `rust-runtime/`.
///
/// This mirrors `CrateSet.ENTIRE_SMITHY_RUNTIME` in `buildSrc/src/main/kotlin/CrateSet.kt`, which
/// is the authoritative list — the same one `copyRuntimeCrates` assembles the release bundle from.
/// It is duplicated here rather than parsed so that drift fails in CI, where
/// [`tests::the_list_matches_crate_set_kt`] reads the Kotlin and compares, rather than failing at
/// release time when the canary is the only thing watching.
///
/// Deliberately absent: the six [`AWS_SDK_RUNTIME`] crates, which publish on the SDK's own train a
/// median 39.6 hours later, so waiting for them would hold the floor tier open for days. Taking the
/// list from `CrateSet` excludes them by construction, and also excludes `aws-smithy-fuzz`, which is
/// a directory under `rust-runtime/` but is in no `CrateSet` list and publishes nothing.
pub const ENTIRE_SMITHY_RUNTIME: &[&str] = &[
    // SMITHY_RUNTIME_COMMON
    "aws-smithy-async",
    "aws-smithy-cbor",
    "aws-smithy-checksums",
    "aws-smithy-compression",
    "aws-smithy-dns",
    "aws-smithy-eventstream",
    "aws-smithy-experimental",
    "aws-smithy-http",
    "aws-smithy-http-client",
    "aws-smithy-json",
    "aws-smithy-legacy-http",
    "aws-smithy-mocks",
    "aws-smithy-observability",
    "aws-smithy-observability-otel",
    "aws-smithy-protocol-test",
    "aws-smithy-query",
    "aws-smithy-runtime",
    "aws-smithy-runtime-api",
    "aws-smithy-runtime-api-macros",
    "aws-smithy-schema",
    "aws-smithy-types",
    "aws-smithy-types-convert",
    "aws-smithy-wasm",
    "aws-smithy-xml",
    // SERVER_SPECIFIC_SMITHY_RUNTIME
    "aws-smithy-http-server",
    "aws-smithy-http-server-metrics",
    "aws-smithy-http-server-metrics-macro",
    "aws-smithy-http-server-python",
    "aws-smithy-http-server-typescript",
    "aws-smithy-legacy-http-server",
];

/// The runtime crates that live in `aws/rust-runtime/` and publish on the SDK's own train.
///
/// Mirrors `CrateSet.AWS_SDK_RUNTIME`, checked against it by
/// [`tests::the_sdk_runtime_list_matches_crate_set_kt`].
///
/// These are deliberately *not* part of [`ENTIRE_SMITHY_RUNTIME`], because [`status`] must not wait
/// for them: they arrive a median 39.6 hours after a smithy-rs release, which would hold the first
/// tier open for days. That exclusion is about what the canary *waits for*, and says nothing about
/// what can break — a consumer resolving two majors of `aws-types` is broken exactly as thoroughly
/// as one resolving two majors of `aws-smithy-types`. So [`crate::lockfile`] considers both lists
/// when attributing a failure.
pub const AWS_SDK_RUNTIME: &[&str] = &[
    "aws-config",
    "aws-credential-types",
    "aws-runtime",
    "aws-runtime-api",
    "aws-sigv4",
    "aws-types",
];

/// A crate and the version the checkout declares for it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct RuntimeCrate {
    pub name: String,
    pub version: String,
}

/// What a release publishes, read out of a checkout at the release tag.
///
/// Drops anything marked `publish = false`: such a crate is never going to appear on crates.io, so
/// waiting for it would hold the first tier open until its timeout. Today that is exactly one of
/// the thirty, `aws-smithy-http-server-typescript`.
pub fn declared(repo_root: &Path) -> anyhow::Result<Vec<RuntimeCrate>> {
    let mut out = Vec::new();
    for name in ENTIRE_SMITHY_RUNTIME {
        let path = repo_root.join("rust-runtime").join(name).join("Cargo.toml");
        let text = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
        let manifest: toml::Value = text
            .parse()
            .map_err(|e| anyhow::anyhow!("parsing {}: {e}", path.display()))?;
        let package = manifest
            .get("package")
            .ok_or_else(|| anyhow::anyhow!("{} has no [package]", path.display()))?;

        if package.get("publish").and_then(|p| p.as_bool()) == Some(false) {
            continue;
        }
        let version = package
            .get("version")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("{} has no package.version", path.display()))?;
        out.push(RuntimeCrate {
            name: (*name).to_string(),
            version: version.to_string(),
        });
    }
    out.sort();
    Ok(out)
}

/// The versions crates.io holds for a crate. Injected so the comparison can be tested without
/// reaching the network, and so a rate limit cannot turn a unit test red.
pub trait PublishedIndex {
    /// Every version crates.io lists for the crate, each with its yanked flag. Empty if the crate
    /// does not exist yet.
    ///
    /// One method returning both, rather than separate version and yank lookups, because each call
    /// is an HTTP request: twenty-nine crates polled every fifteen minutes for three hours is
    /// enough to matter, and crates.io rate-limits.
    fn versions(&self, crate_name: &str) -> anyhow::Result<BTreeMap<String, bool>>;
}

/// Which declared crates crates.io does not yet have, and which are present but yanked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    /// Declared versions crates.io does not list. Empty means publishing has finished.
    pub outstanding: Vec<RuntimeCrate>,
    /// Present, but yanked. Not outstanding — publishing did happen — yet worth surfacing, because
    /// a consumer cannot resolve a yanked version, so the legs will fail where a human would want
    /// to know the cause was a yank rather than a code break.
    pub yanked: Vec<RuntimeCrate>,
    /// How many of the declared crates have arrived, out of how many are expected.
    pub arrived: usize,
    pub expected: usize,
}

impl Status {
    pub fn is_complete(&self) -> bool {
        self.outstanding.is_empty()
    }
}

/// Compare what the tag declares against what crates.io holds.
pub fn status(declared: &[RuntimeCrate], index: &dyn PublishedIndex) -> anyhow::Result<Status> {
    let mut outstanding = Vec::new();
    let mut yanked = Vec::new();

    for want in declared {
        // One lookup per crate, and both answers come out of it.
        match index.versions(&want.name)?.get(&want.version) {
            None => outstanding.push(want.clone()),
            Some(&is_yanked) => {
                // Present, so publishing happened and it is not outstanding. A yank is still worth
                // naming: a consumer cannot resolve a yanked version, so the legs will go red and
                // whoever reads the page needs to know the cause was a yank, not a code break.
                if is_yanked {
                    yanked.push(want.clone());
                }
            }
        }
    }

    Ok(Status {
        arrived: declared.len() - outstanding.len(),
        expected: declared.len(),
        outstanding,
        yanked,
    })
}

/// crates.io, over its HTTP API.
///
/// The *API* rather than the git index, deliberately. The index is what cargo reads and is served
/// from a CDN, so it can lag behind a publish by minutes — which is the propagation race the
/// resolve step's retry exists to absorb. The API answers from the registry's own database, so
/// asking it "is this version published" gets the authoritative answer rather than a cached one.
/// The two disagreeing is normal, and the canary wants the authority here and the cache in the legs
/// (where a consumer's experience is exactly the cached view).
pub struct CratesIo {
    user_agent: String,
}

impl Default for CratesIo {
    fn default() -> Self {
        // crates.io rejects requests without a User-Agent identifying the caller: verified, a bare
        // request gets 403. Naming the project means a human can find us if we misbehave.
        Self {
            user_agent: "smithy-rs-build-canary (https://github.com/smithy-lang/smithy-rs)"
                .to_string(),
        }
    }
}

impl PublishedIndex for CratesIo {
    fn versions(&self, crate_name: &str) -> anyhow::Result<BTreeMap<String, bool>> {
        let url = format!("https://crates.io/api/v1/crates/{crate_name}");
        let out = std::process::Command::new("curl")
            .args([
                "--silent",
                "--show-error",
                "--max-time",
                "30",
                "--write-out",
                "\n%{http_code}",
                "--user-agent",
                &self.user_agent,
                &url,
            ])
            .output()
            .map_err(|e| anyhow::anyhow!("running curl for {crate_name}: {e}"))?;

        let body = String::from_utf8_lossy(&out.stdout);
        let (json, code) = body
            .rsplit_once('\n')
            .ok_or_else(|| anyhow::anyhow!("no status line from crates.io for {crate_name}"))?;

        match code.trim() {
            // A crate nobody has ever published. Not an error: a brand new runtime crate is absent
            // until the release that first publishes it, and that is exactly "outstanding".
            "404" => return Ok(BTreeMap::new()),
            "200" => {}
            other => {
                anyhow::bail!(
                    "crates.io returned {other} for {crate_name}: {}",
                    json.chars().take(200).collect::<String>()
                )
            }
        }

        let parsed: serde_json::Value = serde_json::from_str(json).map_err(|e| {
            anyhow::anyhow!("crates.io sent unparseable JSON for {crate_name}: {e}")
        })?;
        let versions = parsed
            .get("versions")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("no versions array for {crate_name}"))?;

        Ok(versions
            .iter()
            .filter_map(|v| {
                let num = v.get("num")?.as_str()?.to_string();
                let yanked = v.get("yanked").and_then(|y| y.as_bool()).unwrap_or(false);
                Some((num, yanked))
            })
            .collect())
    }
}

/// A snapshot of crates.io, fetched once per crate.
#[derive(Debug, Default)]
pub struct Snapshot {
    /// name -> (version -> yanked)
    crates: BTreeMap<String, BTreeMap<String, bool>>,
}

impl Snapshot {
    pub fn insert(&mut self, name: &str, versions: BTreeMap<String, bool>) {
        self.crates.insert(name.to_string(), versions);
    }
}

impl PublishedIndex for Snapshot {
    fn versions(&self, crate_name: &str) -> anyhow::Result<BTreeMap<String, bool>> {
        Ok(self.crates.get(crate_name).cloned().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(entries: &[(&str, &[&str])]) -> Snapshot {
        let mut s = Snapshot::default();
        for (name, versions) in entries {
            s.insert(
                name,
                versions.iter().map(|v| ((*v).to_string(), false)).collect(),
            );
        }
        s
    }

    fn want(name: &str, version: &str) -> RuntimeCrate {
        RuntimeCrate {
            name: name.to_string(),
            version: version.to_string(),
        }
    }

    /// The list is duplicated from Kotlin, so the duplication has to be checked. This fails in CI
    /// the moment `CrateSet.kt` gains or loses a crate, rather than at release time when the canary
    /// is the only thing looking.
    #[test]
    fn the_list_matches_crate_set_kt() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repo root")
            .to_path_buf();
        let kotlin = std::fs::read_to_string(root.join("buildSrc/src/main/kotlin/CrateSet.kt"))
            .expect("CrateSet.kt should be readable from the repo");

        // ENTIRE_SMITHY_RUNTIME is SMITHY_RUNTIME_COMMON plus SERVER_SPECIFIC_SMITHY_RUNTIME.
        let section = |name: &str| -> Vec<String> {
            let start = kotlin
                .find(&format!("val {name} ="))
                .unwrap_or_else(|| panic!("{name} not found in CrateSet.kt"));
            let body = &kotlin[start..];
            let end = body
                .find(").map")
                .or_else(|| body.find(")\n"))
                .expect("list end");
            body[..end]
                .lines()
                .filter_map(|l| {
                    let l = l.trim();
                    // Entries are either `"name",` or `Crate("name", PROP),`.
                    let l = l.strip_prefix("Crate(").unwrap_or(l);
                    l.strip_prefix('"')?.split('"').next().map(str::to_string)
                })
                .filter(|s| s.starts_with("aws-"))
                .collect()
        };

        let mut from_kotlin = section("SMITHY_RUNTIME_COMMON");
        from_kotlin.extend(section("SERVER_SPECIFIC_SMITHY_RUNTIME"));
        from_kotlin.sort();
        from_kotlin.dedup();

        let mut ours: Vec<String> = ENTIRE_SMITHY_RUNTIME
            .iter()
            .map(|s| s.to_string())
            .collect();
        ours.sort();

        assert_eq!(
            ours, from_kotlin,
            "this list has drifted from CrateSet.ENTIRE_SMITHY_RUNTIME"
        );
        assert_eq!(ours.len(), 30, "24 common + 6 server-specific");
    }

    /// `AWS_SDK_RUNTIME` is duplicated from the same Kotlin and needs the same guard. It is used to
    /// decide whether a duplicated crate in a consumer's graph is one the canary should name, so a
    /// crate added to the SDK runtime and not here would go unattributed on a page.
    #[test]
    fn the_sdk_runtime_list_matches_crate_set_kt() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repo root");
        let kotlin = std::fs::read_to_string(root.join("buildSrc/src/main/kotlin/CrateSet.kt"))
            .expect("CrateSet.kt should be readable from the repo");

        let start = kotlin
            .find("val AWS_SDK_RUNTIME =")
            .expect("AWS_SDK_RUNTIME not found in CrateSet.kt");
        let body = &kotlin[start..];
        let end = body.find(").map").expect("list end");
        let from_kotlin: Vec<String> = body[..end]
            .lines()
            .filter_map(|l| l.trim().strip_prefix('"')?.split('"').next())
            .map(str::to_string)
            .collect();

        assert_eq!(
            AWS_SDK_RUNTIME.to_vec(),
            from_kotlin,
            "this list has drifted from CrateSet.AWS_SDK_RUNTIME"
        );
        assert_eq!(AWS_SDK_RUNTIME.len(), 6);
    }

    /// The two lists must stay disjoint. They are both used as "is this crate under test", and an
    /// overlap would mean a crate was being waited for on the smithy train while also being
    /// documented as arriving on the SDK's.
    #[test]
    fn the_two_published_lists_are_disjoint() {
        let overlap: Vec<&&str> = ENTIRE_SMITHY_RUNTIME
            .iter()
            .filter(|c| AWS_SDK_RUNTIME.contains(c))
            .collect();
        assert!(overlap.is_empty(), "{overlap:?} is in both lists");
    }

    /// Reading the real checkout: the one `publish = false` crate must be dropped, leaving 29.
    #[test]
    fn the_declared_set_drops_unpublishable_crates() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repo root");
        let declared = declared(root).expect("the checkout should be readable");

        assert_eq!(
            declared.len(),
            29,
            "30 in CrateSet minus aws-smithy-http-server-typescript, which is publish = false"
        );
        assert!(!declared.iter().any(|c| c.name.contains("typescript")));
        assert!(declared.iter().all(|c| !c.version.is_empty()));
        // The AWS SDK runtime crates publish on a different train and must not be waited for.
        assert!(!declared.iter().any(|c| c.name == "aws-config"));
        assert!(!declared.iter().any(|c| c.name == "aws-types"));
        // Nor the stub crate that publishes nothing.
        assert!(!declared.iter().any(|c| c.name == "aws-smithy-fuzz"));
    }

    #[test]
    fn nothing_published_means_everything_outstanding() {
        let declared = vec![
            want("aws-smithy-types", "1.9.5"),
            want("aws-smithy-json", "0.65.2"),
        ];
        let s = status(&declared, &snapshot(&[])).unwrap();
        assert_eq!(s.outstanding, declared);
        assert_eq!((s.arrived, s.expected), (0, 2));
        assert!(!s.is_complete());
    }

    #[test]
    fn everything_published_means_complete() {
        let declared = vec![
            want("aws-smithy-types", "1.9.5"),
            want("aws-smithy-json", "0.65.2"),
        ];
        let index = snapshot(&[
            ("aws-smithy-types", &["1.9.4", "1.9.5"]),
            ("aws-smithy-json", &["0.65.2"]),
        ]);
        let s = status(&declared, &index).unwrap();
        assert!(s.is_complete());
        assert_eq!((s.arrived, s.expected), (2, 2));
    }

    /// The partial state the end condition exists to recognise, and the reason it reports *which*
    /// crates are missing rather than only a count.
    #[test]
    fn a_half_published_release_names_what_is_missing() {
        let declared = vec![
            want("aws-smithy-types", "1.9.5"),
            want("aws-smithy-json", "0.65.2"),
            want("aws-smithy-xml", "0.64.2"),
        ];
        let index = snapshot(&[("aws-smithy-types", &["1.9.5"])]);
        let s = status(&declared, &index).unwrap();

        assert_eq!((s.arrived, s.expected), (1, 3));
        assert_eq!(
            s.outstanding
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            ["aws-smithy-json", "aws-smithy-xml"]
        );
    }

    /// Exact membership, not "crates.io is at least as new as us". September 2026 republished
    /// `aws-smithy-types` *downwards* after a yank, 1.7.0 to 1.6.4, so a greater-or-equal test
    /// would have called 1.6.4 arrived while crates.io still only had the yanked 1.7.0.
    #[test]
    fn a_version_published_downwards_is_not_mistaken_for_arrived() {
        let declared = vec![want("aws-smithy-types", "1.6.4")];
        // crates.io holds a *newer* version than the one this tag publishes.
        let index = snapshot(&[("aws-smithy-types", &["1.6.3", "1.7.0"])]);
        let s = status(&declared, &index).unwrap();

        assert_eq!(s.outstanding, declared, "1.6.4 is not on crates.io yet");
        assert!(!s.is_complete());
    }

    /// Nothing about this is a version comparison, so neighbouring version strings cannot be
    /// confused the way a lexical or naive numeric compare would confuse 0.65.1 and 0.65.10.
    #[test]
    fn similar_version_strings_are_not_confused() {
        let declared = vec![want("aws-smithy-json", "0.65.10")];
        let index = snapshot(&[("aws-smithy-json", &["0.65.1"])]);
        assert_eq!(
            status(&declared, &index).unwrap().outstanding,
            declared,
            "0.65.1 is not 0.65.10"
        );
    }

    /// A watcher that starts after publishing began must be correct about what already landed,
    /// which is the property that lets the same computation serve as both "expected" and
    /// "outstanding".
    #[test]
    fn the_predicate_is_idempotent_for_a_late_start() {
        let declared = vec![
            want("aws-smithy-types", "1.9.5"),
            want("aws-smithy-json", "0.65.2"),
        ];
        let index = snapshot(&[("aws-smithy-types", &["1.9.5"])]);

        let first = status(&declared, &index).unwrap();
        let second = status(&declared, &index).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.arrived, 1);
    }
}
