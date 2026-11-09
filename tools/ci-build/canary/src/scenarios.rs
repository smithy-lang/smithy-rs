/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The scenario table, and its expansion into the leg matrix.
//!
//! Scenario × toolchain = one leg. Every leg has its own lockfile and its own verdict.
//!
//! The ten dependency scenarios run at each surviving toolchain, so they give 10 × 5 = 50 legs. The
//! ten build-axis legs are *separate* scenarios rather than extra dimensions on those ten, so they
//! are not multiplied by the toolchain axis and contribute 10 more. **60 legs.**
//!
//! Thirteen manifests back all 60: one per dependency scenario, plus `wasm`, `serialization` and
//! `large-generated-clients`. The build-axis legs that vary only target or profile reuse the
//! `new-project` manifest, which is why 13 manifests produce 60 legs.

use serde::Serialize;

use crate::toolchain::Toolchain;

/// What the leg builds on top of its manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Body {
    /// The `canary-build-check` app: `src/main.rs` as compile surface plus the assertions under
    /// `tests/`. Used where the manifest resolves a client, so there is an API to call.
    CanaryApp,
    /// `fn main() {}`. Used where the point is that the graph *compiles*, not that it runs — the
    /// third-party dependents and the server scenario.
    Stub,
    /// An empty `src/lib.rs` and no binary.
    ///
    /// This exists for the cross-compiled legs. Producing an rlib does not invoke the system linker,
    /// so these targets build with nothing but `rustup target add` — measured: a `bin` target for
    /// `i686-unknown-linux-gnu` fails on `cannot find Scrt1.o` without `gcc-multilib`, while the
    /// same dependencies as a `lib` build clean. That is what lets the exotic legs skip CI's
    /// `cross`, Docker and cross-compiled-OpenSSL machinery entirely.
    StubLib,
}

/// Which of the leg's six commands can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Checks {
    /// `cargo build`, `cargo test --no-run`, `cargo test` — the full verdict set.
    Full,
    /// `cargo build` only, so the leg can yield `COMPILE_FAIL`, `RESOLVE_FAIL` or `INFRA_FAIL` but
    /// never `HARNESS_FAIL` or `RUNTIME_FAIL`.
    ///
    /// Used by the cross-compiled legs: there is no powerpc runner, and running the tests would need
    /// the architecture emulation CI calls "a more complicated setup".
    BuildOnly,
}

/// The cargo profile the leg builds with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    Debug,
    Release,
}

/// One of the ten dependency scenarios. Each has its own manifest, 1:1.
#[derive(Debug, Clone, Copy)]
pub struct Scenario {
    /// Stable id, used as the manifest name and part of the leg id.
    pub id: &'static str,
    /// `(crate, requirement)` exactly as a consumer would declare it. Requirement *ranges*, never
    /// pins: the point is to resolve what a consumer would resolve.
    pub deps: &'static [(&'static str, &'static str)],
    pub body: Body,
    /// What this row is for. Kept in the data so the matrix is self-documenting in the job log.
    pub reaches: &'static str,
}

/// The runner the default (x86_64 Linux) legs use.
pub const DEFAULT_RUNNER: &str = "ubuntu-latest";

/// Runners this repository's CI already uses, and therefore the only ones a canary leg may ask for.
///
/// The canary must not claim coverage the project does not otherwise have: a platform CI never
/// builds is a platform nobody has agreed to support, so a canary failure there is not actionable.
/// This list is the ceiling, enforced by a test.
pub const CI_RUNNERS: &[&str] = &[
    "ubuntu-latest",
    "ubuntu-24.04",
    "ubuntu-22.04",
    "smithy_ubuntu-latest_8-core",
    "ubuntu-24.04-arm",
    "windows-latest",
];

/// A build-axis leg. Not multiplied by the toolchain axis — each is one leg at `stable`.
#[derive(Debug, Clone, Copy)]
pub struct BuildAxisLeg {
    pub id: &'static str,
    /// Which manifest it builds. Most reuse `new-project`.
    pub manifest: &'static str,
    /// The GitHub Actions runner. Must appear in [`CI_RUNNERS`].
    pub runner: &'static str,
    /// `None` means build for the runner's own target, which is how CI covers aarch64 and Windows.
    pub target: Option<&'static str>,
    pub profile: Profile,
    pub body: Body,
    pub reaches: &'static str,
}

/// The ten dependency scenarios.
///
/// The first five are manifests we write. The last five inherit their requirements on us from a
/// third party's `Cargo.toml`, which is the whole point of those rows — patching their feature set
/// or requirements would destroy it.
pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        id: "new-project",
        deps: &[("aws-config", "1"), ("aws-sdk-s3", "1")],
        body: Body::CanaryApp,
        reaches: "the client path — the break that reached customers",
    },
    Scenario {
        id: "server-version-only",
        deps: &[("aws-smithy-http-server-metrics", "0.2")],
        body: Body::Stub,
        reaches: "the server path — a version-only requirement on immutable crates. Reproduces \
                  September's shape: 0.2 resolves 0.2.2, pulling aws-smithy-json 0.63.1 against \
                  aws-smithy-types 1.9.0",
    },
    // The three prior lines all fail E0308 inside aws-config itself, on two majors of
    // aws-smithy-schema in one graph with SharedClientProtocol crossing between them. 1.9.0–1.12.0
    // require aws-smithy-schema ^0.2.0; 1.13.0 moved to ^1.0.0; <=1.8.16 has no direct dependency.
    Scenario {
        id: "prior-aws-config-1-12",
        deps: &[("aws-config", "~1.12"), ("aws-sdk-s3", "1")],
        body: Body::CanaryApp,
        reaches: "a consumer who has not bumped yet",
    },
    Scenario {
        id: "prior-aws-config-1-11",
        deps: &[("aws-config", "~1.11"), ("aws-sdk-s3", "1")],
        body: Body::CanaryApp,
        reaches: "a consumer who has not bumped yet",
    },
    Scenario {
        id: "prior-aws-config-1-10",
        deps: &[("aws-config", "~1.10"), ("aws-sdk-s3", "1")],
        body: Body::CanaryApp,
        reaches: "a consumer who has not bumped yet",
    },
    Scenario {
        id: "aws-msk-iam-sasl-signer",
        deps: &[("aws-msk-iam-sasl-signer", "1")],
        body: Body::Stub,
        reaches: "a dependent whose requirements on us are frozen",
    },
    Scenario {
        id: "aws-rds-signer",
        deps: &[("aws-rds-signer", "0.2")],
        body: Body::Stub,
        reaches: "a dependent whose requirements on us are frozen",
    },
    Scenario {
        id: "tower-sessions-dynamodb-store",
        deps: &[("tower-sessions-dynamodb-store", "0.3")],
        body: Body::Stub,
        reaches: "a dependent whose requirements on us are frozen, community-owned",
    },
    Scenario {
        id: "aws-esdk",
        deps: &[("aws-esdk", "1")],
        body: Body::Stub,
        reaches: "a dependent whose requirements on us are frozen",
    },
    // Red at admission on every toolchain. Both defects are in ssstar's own source: an E0432 from a
    // feature-gated module it imports without enabling the feature, and E0283 ambiguities at
    // extract.rs:294. Source-level errors do not depend on the compiler version.
    Scenario {
        id: "ssstar",
        deps: &[("ssstar", "0.7")],
        body: Body::Stub,
        reaches: "a dependent whose requirements on us are frozen — red at admission, see above",
    },
];

/// The ten build-axis legs.
///
/// Every platform here is one this repository's CI already builds — see [`CI_RUNNERS`] for why that
/// is a hard constraint rather than a preference. Two consequences of applying it:
///
/// * **macOS is gone.** `macos`, `darwin` and `apple` appear nowhere in `.github/workflows/` or
///   `tools/ci-scripts/`. The project does not build for macOS, so the canary must not either: a red
///   macOS leg would name a platform nobody has agreed to support, which is not actionable.
/// * **musl is aarch64, not x86_64.** CI's musl coverage is `aarch64-unknown-linux-musl`, added by
///   `rustup target add` in the existing `Canary (aarch64)` job. The capability under test — static
///   linking, musl's own runtime, per-target C and assembly in `aws-lc-sys` and `ring` — is the same,
///   so the leg stays and takes the triple CI actually uses.
pub const BUILD_AXIS: &[BuildAxisLeg] = &[
    BuildAxisLeg {
        id: "musl",
        manifest: "new-project",
        // Matches the existing `Canary (aarch64)` job, which builds this triple on an ARM runner.
        runner: "ubuntu-24.04-arm",
        target: Some("aarch64-unknown-linux-musl"),
        profile: Profile::Debug,
        body: Body::CanaryApp,
        reaches:
            "containers, Alpine, Lambda provided.al2; per-target C and assembly in aws-lc-sys \
                  and ring; musl's own runtime",
    },
    BuildAxisLeg {
        id: "aarch64",
        manifest: "new-project",
        // Native, as CI does it, so the leg can run the assertions rather than only cross-compile.
        runner: "ubuntu-24.04-arm",
        target: None,
        profile: Profile::Debug,
        body: Body::CanaryApp,
        reaches: "an architecture a large share of consumers deploy on, including defects an x86 \
                  host cannot reach",
    },
    BuildAxisLeg {
        id: "windows",
        manifest: "new-project",
        // A real Windows runner, as `Rust Tests on Windows` uses. The design had this leg
        // cross-compiled via cargo-xwin and so unable to execute a PE; on windows-latest it runs.
        runner: "windows-latest",
        target: None,
        profile: Profile::Debug,
        body: Body::CanaryApp,
        reaches: "schannel and the Windows root-certificate path, which no Linux target compiles",
    },
    BuildAxisLeg {
        id: "release-profile",
        manifest: "new-project",
        runner: DEFAULT_RUNNER,
        target: None,
        profile: Profile::Release,
        body: Body::CanaryApp,
        reaches: "the profile consumers ship, where codegen and feature unification differ",
    },
    BuildAxisLeg {
        id: "wasm-native",
        manifest: "wasm",
        runner: DEFAULT_RUNNER,
        target: None,
        profile: Profile::Debug,
        body: Body::Stub,
        reaches: "aws-smithy-wasm, a crate we publish and otherwise never compile against the \
                  registry",
    },
    BuildAxisLeg {
        id: "wasm-wasip2",
        manifest: "wasm",
        runner: DEFAULT_RUNNER,
        target: Some("wasm32-wasip2"),
        profile: Profile::Debug,
        body: Body::Stub,
        reaches: "aws-smithy-wasm built for wasm32-wasip2; needs no cross and no nightly",
    },
    BuildAxisLeg {
        id: "large-generated-clients",
        manifest: "large-generated-clients",
        runner: DEFAULT_RUNNER,
        target: None,
        profile: Profile::Debug,
        body: Body::Stub,
        reaches: "1,233,084 lines of generated code against S3's 252,379, plus a streaming-heavy \
                  codegen surface",
    },
    // The three exotic targets CI's `Exotic platform support` job builds. They use the narrow
    // `serialization` manifest, not the client path — see BUILD_AXIS_MANIFESTS for why that is
    // forced rather than chosen. `ubuntu-24.04` matches CI's pin.
    BuildAxisLeg {
        id: "i686",
        manifest: "serialization",
        runner: "ubuntu-24.04",
        target: Some("i686-unknown-linux-gnu"),
        profile: Profile::Debug,
        body: Body::StubLib,
        reaches: "a 32-bit pointer width, where a cast or a length computation can overflow",
    },
    BuildAxisLeg {
        id: "powerpc",
        manifest: "serialization",
        runner: "ubuntu-24.04",
        target: Some("powerpc-unknown-linux-gnu"),
        profile: Profile::Debug,
        body: Body::StubLib,
        reaches:
            "big-endian at a 32-bit pointer width — byte-order bugs no x86 or aarch64 leg can \
                  reach, combined with 32-bit arithmetic",
    },
    BuildAxisLeg {
        id: "powerpc64",
        manifest: "serialization",
        runner: "ubuntu-24.04",
        target: Some("powerpc64-unknown-linux-gnu"),
        profile: Profile::Debug,
        body: Body::StubLib,
        reaches: "big-endian, the one class of defect every other leg in the matrix is blind to",
    },
];

/// The two manifests that exist only for build-axis legs.
pub const BUILD_AXIS_MANIFESTS: &[Scenario] = &[
    Scenario {
        id: "wasm",
        deps: &[("aws-smithy-wasm", "0.2")],
        body: Body::Stub,
        reaches: "the wasm legs",
    },
    Scenario {
        id: "large-generated-clients",
        deps: &[("aws-sdk-ec2", "1")],
        body: Body::Stub,
        reaches: "the large-generated-clients leg",
    },
    // The cross-compiled legs cannot use the client path, and this is forced rather than chosen.
    // CI excludes `aws-smithy-http-client` on all three exotic targets ("due to FIPS related feature
    // flags and aws-lc-fips target support"), and it reaches a consumer manifest *transitively* via
    // aws-smithy-runtime — verified: `aws-config = "1"` + `aws-sdk-s3 = "1"` resolves
    // aws-smithy-http-client 1.5.2. `--exclude` only works on workspace members, so a client-shaped
    // exotic leg would have to build the one crate the project deliberately skips there, and would
    // be red at admission.
    //
    // What is left is the serialization surface, which is where byte-order and pointer-width defects
    // actually live: number, blob and datetime encoding in aws-smithy-types, and the binary framing
    // in aws-smithy-eventstream. Verified to resolve 46 packages with zero aws-smithy-http-client
    // and zero openssl, which is also why these legs need no OpenSSL bootstrap.
    Scenario {
        id: "serialization",
        deps: &[
            ("aws-smithy-types", "1"),
            ("aws-smithy-json", "0.65"),
            ("aws-smithy-xml", "0.64"),
            ("aws-smithy-eventstream", "0.61"),
        ],
        body: Body::StubLib,
        reaches: "the cross-compiled legs: encoding and framing, on targets whose endianness or \
                  pointer width differ from every other leg",
    },
];

/// One unit of work: a manifest, a toolchain, a target, a profile, a body, and a verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Leg {
    /// Unique across the matrix. Used as the GitHub Actions matrix key and as a metric dimension.
    pub id: String,
    /// Which of the thirteen manifests to render.
    pub manifest: String,
    /// `stable`, `stable-1`, … The build-axis legs are all `stable`.
    pub toolchain: String,
    /// The concrete version handed to `rustup`, so a leg pins rather than drifting mid-window.
    pub toolchain_version: String,
    /// The GitHub Actions runner, fed straight to `runs-on`.
    pub runner: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub profile: Profile,
    pub body: Body,
    /// Which commands the leg runs, and so which verdicts it can produce.
    pub checks: Checks,
}

/// A cross-compiled leg builds but cannot execute, so it runs `cargo build` and stops.
fn checks_for(body: Body, target: Option<&str>) -> Checks {
    match (body, target) {
        (Body::StubLib, _) => Checks::BuildOnly,
        // wasm32-wasip2 is cross-compiled too, and nothing here runs a wasm engine.
        (_, Some(t)) if t.starts_with("wasm32") => Checks::BuildOnly,
        _ => Checks::Full,
    }
}

/// Expand the scenario table into the leg matrix for a set of surviving toolchains.
///
/// Dependency scenarios are multiplied by the toolchain axis; build-axis legs are not, and always
/// run at `stable`.
pub fn legs(toolchains: &[Toolchain]) -> Vec<Leg> {
    let stable = toolchains
        .first()
        .expect("clamp() guarantees at least one toolchain");

    let dependency = SCENARIOS.iter().flat_map(|s| {
        toolchains.iter().map(move |tc| Leg {
            id: format!("{}--{}", s.id, tc.name),
            manifest: s.id.to_string(),
            toolchain: tc.name.clone(),
            toolchain_version: tc.version.to_string(),
            runner: DEFAULT_RUNNER.to_string(),
            target: None,
            profile: Profile::Debug,
            body: s.body,
            checks: checks_for(s.body, None),
        })
    });

    let build_axis = BUILD_AXIS.iter().map(|b| Leg {
        id: b.id.to_string(),
        manifest: b.manifest.to_string(),
        toolchain: stable.name.clone(),
        toolchain_version: stable.version.to_string(),
        runner: b.runner.to_string(),
        target: b.target.map(str::to_string),
        profile: b.profile,
        body: b.body,
        checks: checks_for(b.body, b.target),
    });

    dependency.chain(build_axis).collect()
}

/// Every manifest the matrix needs, by id.
pub fn manifests() -> Vec<&'static Scenario> {
    SCENARIOS.iter().chain(BUILD_AXIS_MANIFESTS).collect()
}

/// Look up a manifest by id.
pub fn manifest(id: &str) -> Option<&'static Scenario> {
    manifests().into_iter().find(|s| s.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolchain::{self, Version};
    use std::collections::HashSet;

    fn today() -> Vec<Toolchain> {
        toolchain::resolve(Version::new(1, 99, 1), Version::new(1, 94, 1))
            .unwrap()
            .kept
    }

    #[test]
    fn the_table_is_ten_scenarios_and_ten_build_axis_legs() {
        assert_eq!(SCENARIOS.len(), 10);
        assert_eq!(BUILD_AXIS.len(), 10);
    }

    /// The headline number, cross-checked the other way round: 10 × 5 + 10.
    #[test]
    fn five_toolchains_give_sixty_legs() {
        let tcs = today();
        assert_eq!(tcs.len(), 5);
        let legs = legs(&tcs);
        assert_eq!(legs.len(), 60);

        let dependency = legs
            .iter()
            .filter(|l| !BUILD_AXIS.iter().any(|b| b.id == l.id))
            .count();
        assert_eq!(dependency, 50, "10 scenarios × 5 toolchains");
        assert_eq!(legs.len() - dependency, 10, "build-axis legs");
    }

    /// The second cross-check: 9 + 9×5 + 2 + 1 + 3.
    #[test]
    fn the_manifest_to_leg_mapping_also_gives_sixty() {
        let legs = legs(&today());
        let count = |m: &str| legs.iter().filter(|l| l.manifest == m).count();

        assert_eq!(count("new-project"), 9, "5 toolchain legs + 4 build-axis");
        for s in SCENARIOS.iter().filter(|s| s.id != "new-project") {
            assert_eq!(count(s.id), 5, "{} should be one leg per toolchain", s.id);
        }
        assert_eq!(count("wasm"), 2);
        assert_eq!(count("large-generated-clients"), 1);
        assert_eq!(count("serialization"), 3, "i686, powerpc, powerpc64");
        assert_eq!(9 + 9 * 5 + 2 + 1 + 3, legs.len());
    }

    /// The canary must not claim coverage the project does not have. A platform CI never builds is a
    /// platform nobody has agreed to support, so a canary failure there is not actionable — and it
    /// would page the oncall about it.
    #[test]
    fn every_leg_runs_on_a_runner_ci_already_uses() {
        for leg in legs(&today()) {
            assert!(
                CI_RUNNERS.contains(&leg.runner.as_str()),
                "leg {} wants runner {:?}, which this repository's CI does not use",
                leg.id,
                leg.runner
            );
        }
    }

    /// Guards the specific removal rather than only the general rule, because this one took finding:
    /// `macos`, `darwin` and `apple` appear nowhere in .github/workflows/ or tools/ci-scripts/.
    #[test]
    fn there_is_no_macos_leg() {
        assert!(
            !BUILD_AXIS.iter().any(|b| b.id == "macos"
                || b.runner.contains("macos")
                || b.target.is_some_and(|t| t.contains("apple"))),
            "CI does not build for macOS, so the canary must not either"
        );
    }

    /// CI's musl coverage is aarch64, added by `rustup target add` in the existing Canary (aarch64)
    /// job. An x86_64-musl leg would be testing a triple nobody else builds.
    #[test]
    fn musl_uses_the_triple_ci_uses() {
        let musl = BUILD_AXIS.iter().find(|b| b.id == "musl").unwrap();
        assert_eq!(musl.target, Some("aarch64-unknown-linux-musl"));
        assert_eq!(musl.runner, "ubuntu-24.04-arm");
    }

    /// The three exotic targets CI builds. Big-endian is the point: no other leg in the matrix can
    /// reach a byte-order defect.
    #[test]
    fn the_exotic_targets_ci_builds_are_covered() {
        let exotic: Vec<_> = BUILD_AXIS
            .iter()
            .filter(|b| b.manifest == "serialization")
            .map(|b| b.target.unwrap())
            .collect();
        assert_eq!(
            exotic,
            [
                "i686-unknown-linux-gnu",
                "powerpc-unknown-linux-gnu",
                "powerpc64-unknown-linux-gnu"
            ]
        );
    }

    /// Forced, not chosen: CI excludes `aws-smithy-http-client` on every exotic target, and it
    /// reaches a consumer manifest transitively via aws-smithy-runtime, where `--exclude` cannot
    /// touch it. A client-shaped exotic leg would be red at admission.
    #[test]
    fn exotic_legs_avoid_the_client_path() {
        let ser = manifest("serialization").unwrap();
        for (crate_name, _) in ser.deps {
            assert!(
                !crate_name.starts_with("aws-config")
                    && !crate_name.starts_with("aws-sdk-")
                    && *crate_name != "aws-smithy-http-client",
                "the serialization manifest must not reach the client path, found {crate_name}"
            );
        }
        for leg in BUILD_AXIS.iter().filter(|b| b.manifest == "serialization") {
            assert_eq!(leg.body, Body::StubLib, "{} must be lib-only", leg.id);
        }
    }

    /// A cross-compiled leg builds but cannot execute, so it must not claim a verdict it cannot
    /// produce. Encoding this stops the leg runner attempting `cargo test` where nothing can run it.
    #[test]
    fn cross_compiled_legs_are_build_only() {
        for leg in legs(&today()) {
            let cross = leg.target.as_deref().is_some_and(|t| {
                t.starts_with("powerpc") || t.starts_with("i686") || t.starts_with("wasm32")
            });
            if cross {
                assert_eq!(
                    leg.checks,
                    Checks::BuildOnly,
                    "{} is cross-compiled",
                    leg.id
                );
            }
        }
        // Native legs, including aarch64 and Windows on their own runners, run everything.
        for id in [
            "new-project--stable",
            "aarch64",
            "windows",
            "release-profile",
        ] {
            let leg = legs(&today()).into_iter().find(|l| l.id == id).unwrap();
            assert_eq!(leg.checks, Checks::Full, "{id} runs natively");
        }
    }

    #[test]
    fn thirteen_manifests_back_the_matrix() {
        assert_eq!(manifests().len(), 13);
        let legs = legs(&today());
        let used: HashSet<_> = legs.iter().map(|l| l.manifest.as_str()).collect();
        assert_eq!(used.len(), 13, "every manifest is reachable from some leg");
    }

    #[test]
    fn leg_ids_are_unique() {
        let legs = legs(&today());
        let ids: HashSet<_> = legs.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids.len(), legs.len(), "leg ids must be unique");
    }

    /// Dropping toolchains must drop exactly the dependency legs, never a build-axis leg — those
    /// always run at `stable`, which survives any clamp.
    #[test]
    fn clamping_the_span_only_removes_dependency_legs() {
        let four = toolchain::resolve(Version::new(1, 99, 1), Version::new(1, 96, 0))
            .unwrap()
            .kept;
        let legs = legs(&four);
        assert_eq!(legs.len(), 10 * 4 + 10);
        assert_eq!(
            legs.iter()
                .filter(|l| BUILD_AXIS.iter().any(|b| b.id == l.id))
                .count(),
            10
        );
    }

    #[test]
    fn one_surviving_toolchain_still_yields_a_usable_matrix() {
        let one = toolchain::resolve(Version::new(1, 99, 1), Version::new(1, 99, 0))
            .unwrap()
            .kept;
        assert_eq!(legs(&one).len(), 10 + 10);
    }

    /// Four scenarios resolve a client and so carry the app body; the other six are stubs. The split
    /// decides how much of the matrix can report a run-phase verdict at all: a stub leg has no
    /// assertions, so it can only ever say `COMPILE_FAIL`, `RESOLVE_FAIL` or `INFRA_FAIL`.
    #[test]
    fn four_scenarios_are_client_shaped() {
        let app: Vec<_> = SCENARIOS
            .iter()
            .filter(|s| s.body == Body::CanaryApp)
            .map(|s| s.id)
            .collect();
        assert_eq!(
            app,
            [
                "new-project",
                "prior-aws-config-1-12",
                "prior-aws-config-1-11",
                "prior-aws-config-1-10"
            ]
        );
    }

    /// Requirements must be ranges, never pins. A pinned manifest stops resolving what a consumer
    /// would resolve, which is the entire premise of the scenario axis.
    #[test]
    fn every_requirement_is_a_range() {
        for s in manifests() {
            for (crate_name, req) in s.deps {
                assert!(
                    !req.starts_with('=') && !req.contains(','),
                    "{}: {crate_name} = {req:?} is a pin, not a range",
                    s.id
                );
            }
        }
    }

    #[test]
    fn manifest_lookup_covers_build_axis_manifests() {
        assert!(manifest("wasm").is_some());
        assert!(manifest("large-generated-clients").is_some());
        assert!(manifest("new-project").is_some());
        assert!(manifest("nope").is_none());
    }
}
