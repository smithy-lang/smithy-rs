/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The scenario table, and its expansion into the leg matrix.
//!
//! Scenario × toolchain = one leg. Every leg has its own lockfile and its own verdict.
//!
//! The six dependency scenarios run at each surviving toolchain, so they give 6 × 5 = 30 legs. The
//! ten build-axis legs are *separate* scenarios rather than extra dimensions on those six, so they
//! are not multiplied by the toolchain axis and contribute 10 more. **40 legs.**
//!
//! Nine manifests back all 40: one per dependency scenario, plus `wasm`, `serialization` and
//! `large-generated-clients`. The build-axis legs that vary only target or profile reuse the
//! `new-project` manifest, which is why 9 manifests produce 40 legs.
//!
//! It was ten scenarios and sixty legs. Four rows are commented out in the table below, each with
//! its reason and what restoring it is gated on: they are red against the crates published today,
//! and a leg that is red on arrival costs the floor tier its remaining twenty-three hours on every
//! release rather than costing one leg of coverage.

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

/// One of the dependency scenarios. Each has its own manifest, 1:1.
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
    /// Set while a break is deliberately rigged into this manifest, saying why.
    ///
    /// `None` on every row in normal operation, and a release where it is set anywhere is a
    /// release being used to test the paging chain rather than to check the crates.
    ///
    /// # What it changes, and what it deliberately does not
    ///
    /// A rigged leg still resolves, still builds, still reaches a verdict, still records that
    /// verdict, and still cuts a ticket. Two things differ:
    ///
    /// * **It does not stop the 24-hour window.** The stop rule reads "the team has already been
    ///   paged, so later hours would dedupe into the same ticket and buy nothing". For a break we
    ///   rigged ourselves that reasoning inverts: the page was about something fake, and the
    ///   remaining hours would still catch something real. See [`crate::leg::Report::exit_code`].
    /// * **It pages on its own series.** `Red {Tag, Induced=true}` rather than `Red {Tag}`, so a
    ///   real break landing in the same release cuts its own ticket instead of deduping into the
    ///   test's. See [`crate::metric::datapoints`].
    ///
    /// # The cost of being wrong about it
    ///
    /// The marker is per *manifest*, so if a release genuinely breaks the rigged manifest while a
    /// test is running, that real break inherits both changes: it would not stop the window, and it
    /// would page as induced. The first is the harmless direction — not stopping means more
    /// checking, never less. The second is real, and it is why the marker is meant to live for one
    /// release and be withdrawn in a commit, rather than sitting in the table.
    ///
    /// Every other manifest is unaffected, which is what bounds the exposure.
    pub induced: Option<&'static str>,
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

/// The dependency scenarios: six live, four withdrawn and commented in place.
///
/// The first two are manifests we write. The rest inherit their requirements on us from a third
/// party's `Cargo.toml`, which is the whole point of those rows — patching their feature set or
/// requirements would destroy it.
///
/// Each withdrawn row carries its own reason and the condition for putting it back. They are
/// commented rather than deleted because every one of them is the right row to be checking; what
/// disqualifies them is that they are red *now*, and a standing red leg makes the floor tier's
/// stop rule fire on arrival.
pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        id: "new-project",
        deps: &[("aws-config", "1"), ("aws-sdk-s3", "1")],
        body: Body::CanaryApp,
        reaches: "the client path — the break that reached customers",
        induced: None,
    },
    Scenario {
        id: "server-version-only",
        deps: &[("aws-smithy-http-server-metrics", "0.2")],
        body: Body::Stub,
        reaches: "the server path — a version-only requirement on immutable crates. Reproduces \
                  September's shape: 0.2 resolves 0.2.2, pulling aws-smithy-json 0.63.1 against \
                  aws-smithy-types 1.9.0",
        induced: None,
    },
    // ----------------------------------------------------------------------------------------
    // WITHDRAWN: three rows that are red against the crates published today.
    //
    // All three fail E0308 inside aws-config itself, on two majors of aws-smithy-schema in one
    // graph with SharedClientProtocol crossing between them. 1.9.0–1.12.0 require
    // aws-smithy-schema ^0.2.0; 1.13.0 moved to ^1.0.0; <=1.8.16 has no direct dependency.
    // Measured on five toolchains each: fifteen legs, every one COMPILE_FAIL at E0308.
    //
    // They are commented out rather than deleted because **the break is real and is not ours to
    // wish away**: a consumer whose manifest says `aws-config = "~1.10"` and `aws-sdk-s3 = "1"`
    // cannot compile today. That is exactly the class of defect this canary exists to catch, so
    // deleting the rows would be deleting the finding.
    //
    // Why they cannot stay switched on: a floor hour asks "did any dispatched run for this tag
    // conclude failure?", and a standing red leg answers yes at hour one of every release. The
    // remaining twenty-three hours are then skipped — so these fifteen legs, by reporting a break
    // that is already known, cost the canary its ability to detect a new one. Red has to be
    // exceptional for that stop rule to mean anything.
    //
    // Restoring them is gated on the incompatibility being fixed, not on a decision about the
    // canary. Uncomment, run `gen-matrix`, and the counts in the tests below move with them.
    // ----------------------------------------------------------------------------------------
    //
    // Scenario {
    //     id: "prior-aws-config-1-12",
    //     deps: &[("aws-config", "~1.12"), ("aws-sdk-s3", "1")],
    //     body: Body::CanaryApp,
    //     reaches: "a consumer who has not bumped yet",
    //     induced: None,
    // },
    // Scenario {
    //     id: "prior-aws-config-1-11",
    //     deps: &[("aws-config", "~1.11"), ("aws-sdk-s3", "1")],
    //     body: Body::CanaryApp,
    //     reaches: "a consumer who has not bumped yet",
    //     induced: None,
    // },
    // Scenario {
    //     id: "prior-aws-config-1-10",
    //     deps: &[("aws-config", "~1.10"), ("aws-sdk-s3", "1")],
    //     body: Body::CanaryApp,
    //     reaches: "a consumer who has not bumped yet",
    //     induced: None,
    // },
    Scenario {
        id: "aws-msk-iam-sasl-signer",
        deps: &[("aws-msk-iam-sasl-signer", "1")],
        body: Body::Stub,
        reaches: "a dependent whose requirements on us are frozen",
        induced: None,
    },
    Scenario {
        id: "aws-rds-signer",
        deps: &[("aws-rds-signer", "0.2")],
        body: Body::Stub,
        reaches: "a dependent whose requirements on us are frozen",
        induced: None,
    },
    Scenario {
        id: "tower-sessions-dynamodb-store",
        deps: &[("tower-sessions-dynamodb-store", "0.3")],
        body: Body::Stub,
        reaches: "a dependent whose requirements on us are frozen, community-owned",
        induced: None,
    },
    Scenario {
        id: "aws-esdk",
        deps: &[("aws-esdk", "1")],
        body: Body::Stub,
        reaches: "a dependent whose requirements on us are frozen",
        induced: None,
    },
    // ----------------------------------------------------------------------------------------
    // WITHDRAWN: red at admission on every toolchain, for a reason nobody here can fix.
    //
    // Both defects are in ssstar's own source: an E0432 from a feature-gated module it imports
    // without enabling the feature, and E0283 ambiguities at extract.rs:294. Source-level errors
    // do not depend on the compiler version, which is why all five toolchains fail identically.
    //
    // This one differs from the three above in who can act on it. Those are a break in crates this
    // repository publishes; this is a break in a third party's code, so no release from here will
    // ever turn it green. It would have gone red on every release forever, taking the floor tier's
    // remaining twenty-three hours with it each time.
    //
    // Kept commented rather than deleted because the row is still the right row — a dependent
    // whose requirements on us are frozen is a real shape worth checking. Restoring it is gated on
    // ssstar publishing a version that compiles.
    // ----------------------------------------------------------------------------------------
    //
    // Scenario {
    //     id: "ssstar",
    //     deps: &[("ssstar", "0.7")],
    //     body: Body::Stub,
    //     reaches: "a dependent whose requirements on us are frozen — red at admission",
    //     induced: None,
    // },
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

/// The three manifests that exist only for build-axis legs.
pub const BUILD_AXIS_MANIFESTS: &[Scenario] = &[
    Scenario {
        id: "wasm",
        deps: &[("aws-smithy-wasm", "0.2")],
        body: Body::Stub,
        reaches: "the wasm legs",
        induced: None,
    },
    Scenario {
        id: "large-generated-clients",
        deps: &[("aws-sdk-ec2", "1")],
        body: Body::Stub,
        reaches: "the large-generated-clients leg",
        induced: None,
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
        induced: None,
    },
];

/// One unit of work: a manifest, a toolchain, a target, a profile, a body, and a verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Leg {
    /// Unique across the matrix. Used as the GitHub Actions matrix key and as a metric dimension.
    pub id: String,
    /// Which manifest to render, by id.
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
    /// Inherited from the manifest this leg builds: see [`Scenario::induced`].
    ///
    /// Omitted from the matrix JSON when absent, so the ordinary matrix is byte-for-byte what it
    /// was before this existed, and a rigged release's matrix says so in the job log.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub induced: Option<String>,
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
            induced: s.induced.map(str::to_string),
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
        // From the manifest, not from the build-axis row, because that is where a rigged break
        // lives. Four build-axis legs share `new-project`, so rigging it rigs them too — which is
        // correct: they build the manifest that was rigged.
        induced: induced(b.manifest).map(str::to_string),
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

/// Why a manifest is rigged, if it is. See [`Scenario::induced`].
pub fn induced(manifest_id: &str) -> Option<&'static str> {
    manifest(manifest_id).and_then(|s| s.induced)
}

/// Every rigged manifest, by id.
///
/// Exists so the state is readable in one call: during the stage that tests the paging chain this
/// should name exactly one manifest, and in normal operation none.
pub fn induced_manifests() -> Vec<&'static str> {
    manifests()
        .into_iter()
        .filter(|s| s.induced.is_some())
        .map(|s| s.id)
        .collect()
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
    fn the_table_is_six_scenarios_and_ten_build_axis_legs() {
        assert_eq!(SCENARIOS.len(), 6, "four rows are withdrawn; see the table");
        assert_eq!(BUILD_AXIS.len(), 10);
    }

    /// The headline number, cross-checked the other way round: 6 × 5 + 10.
    #[test]
    fn five_toolchains_give_forty_legs() {
        let tcs = today();
        assert_eq!(tcs.len(), 5);
        let legs = legs(&tcs);
        assert_eq!(legs.len(), 40);

        let dependency = legs
            .iter()
            .filter(|l| !BUILD_AXIS.iter().any(|b| b.id == l.id))
            .count();
        assert_eq!(dependency, 30, "6 scenarios × 5 toolchains");
        assert_eq!(legs.len() - dependency, 10, "build-axis legs");
    }

    /// The reason the four rows had to go, stated as a property rather than left in a comment.
    ///
    /// A floor hour asks whether any dispatched run for this tag concluded `failure`, so one leg
    /// that is red on arrival answers yes at hour one of every release and the remaining
    /// twenty-three hours are skipped. A row known to be red against the crates published today
    /// therefore does not cost one leg of coverage, it costs the rest of the day — which is why
    /// `ssstar` and the three `prior-aws-config-*` rows are commented out above rather than left
    /// on with their failures tolerated.
    ///
    /// This cannot assert that the live table is green, which would need a network and twelve
    /// minutes. It asserts the weaker thing that is still worth having: that nobody has quietly
    /// put one of those four ids back without also dealing with the break.
    #[test]
    fn no_known_red_row_is_switched_back_on() {
        const KNOWN_RED: &[&str] = &[
            "ssstar",
            "prior-aws-config-1-12",
            "prior-aws-config-1-11",
            "prior-aws-config-1-10",
        ];
        for s in manifests() {
            assert!(
                !KNOWN_RED.contains(&s.id),
                "{} is red against today's published crates: switching it back on costs the floor \
                 tier its remaining twenty-three hours on every release, so it has to come back \
                 together with the fix, and this list has to lose the entry",
                s.id
            );
        }
    }

    /// The second cross-check: 9 + 5×5 + 2 + 1 + 3.
    #[test]
    fn the_manifest_to_leg_mapping_also_gives_forty() {
        let legs = legs(&today());
        let count = |m: &str| legs.iter().filter(|l| l.manifest == m).count();

        assert_eq!(count("new-project"), 9, "5 toolchain legs + 4 build-axis");
        for s in SCENARIOS.iter().filter(|s| s.id != "new-project") {
            assert_eq!(count(s.id), 5, "{} should be one leg per toolchain", s.id);
        }
        assert_eq!(count("wasm"), 2);
        assert_eq!(count("large-generated-clients"), 1);
        assert_eq!(count("serialization"), 3, "i686, powerpc, powerpc64");
        assert_eq!(9 + 5 * 5 + 2 + 1 + 3, legs.len());
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
    fn nine_manifests_back_the_matrix() {
        assert_eq!(manifests().len(), 9);
        let legs = legs(&today());
        let used: HashSet<_> = legs.iter().map(|l| l.manifest.as_str()).collect();
        assert_eq!(used.len(), 9, "every manifest is reachable from some leg");
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
        assert_eq!(legs.len(), 6 * 4 + 10);
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
        assert_eq!(legs(&one).len(), 6 + 10);
    }

    /// One scenario resolves a client and so carries the app body; the other five are stubs.
    ///
    /// The split decides how much of the matrix can report a run-phase verdict at all: a stub leg
    /// has no assertions, so it can only ever say `COMPILE_FAIL`, `RESOLVE_FAIL` or `INFRA_FAIL`.
    ///
    /// It used to be four, and withdrawing the three `prior-aws-config-*` rows is what took it to
    /// one. Nine legs of forty now carry assertions — `new-project` at five toolchains plus the
    /// four build-axis legs that reuse its manifest — where before it was twenty-four of sixty.
    /// That is the real cost of the withdrawal, and it is concentrated: `new-project` is now the
    /// only dependency row that can produce a `RUNTIME_FAIL` or `HARNESS_FAIL` at all.
    #[test]
    fn one_scenario_is_client_shaped() {
        let app: Vec<_> = SCENARIOS
            .iter()
            .filter(|s| s.body == Body::CanaryApp)
            .map(|s| s.id)
            .collect();
        assert_eq!(app, ["new-project"]);

        // And the legs that can run assertions, which is what the split is actually about.
        let asserting = legs(&today())
            .iter()
            .filter(|l| l.body == Body::CanaryApp)
            .count();
        assert_eq!(
            asserting, 9,
            "5 toolchain legs + musl, aarch64, windows, release-profile"
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

    /// The withdrawal check. A manifest is rigged for exactly one release and withdrawn in a commit
    /// before the next; one left behind turns the next release's observation into noise, and two at
    /// once share nothing useful — both breaks would be in flight at the same time and neither
    /// would answer the question the other was meant to.
    ///
    /// Not an assertion that nothing is rigged, which would block the very commit that rigs one.
    #[test]
    fn at_most_one_manifest_is_rigged_at_a_time() {
        let rigged = induced_manifests();
        assert!(
            rigged.len() <= 1,
            "{rigged:?} are all rigged; one release tests one break, so the others should have \
             been withdrawn"
        );
        for id in &rigged {
            let why = induced(id).expect("it is in the list because it is rigged");
            assert!(
                !why.trim().is_empty(),
                "{id} is rigged without saying why, and the reason is the only thing that tells \
                 the next reader this was deliberate"
            );
        }
    }

    /// Four build-axis legs build `new-project`, so rigging that manifest has to rig them too.
    ///
    /// Vacuous while nothing is rigged — both sides are `None` — and that is the point: it comes
    /// alive in exactly the state it guards. Had the build-axis legs been given a hardcoded `None`
    /// instead of inheriting, `musl`, `aarch64`, `windows` and `release-profile` would each have
    /// gone red without the marker and stopped the window — reintroducing, on four legs out of
    /// forty, the very cost the marker exists to avoid. Verified for real by rigging a manifest and
    /// reading the matrix back, not by this test.
    #[test]
    fn every_leg_of_a_rigged_manifest_inherits_the_marker() {
        for leg in legs(&today()) {
            assert_eq!(
                leg.induced.as_deref(),
                induced(&leg.manifest),
                "leg {} and manifest {} disagree about being rigged",
                leg.id,
                leg.manifest
            );
        }
    }

    /// The matrix is consumed as a GitHub Actions matrix, so adding a field to it is a change to
    /// something a workflow reads. Omitted when absent, which keeps the ordinary matrix exactly
    /// what it was and makes a rigged release's matrix say so in the job log.
    #[test]
    fn the_marker_is_absent_from_the_matrix_json_unless_set() {
        let json = serde_json::to_string(&legs(&today())).unwrap();
        assert_eq!(
            json.contains("induced"),
            !induced_manifests().is_empty(),
            "the field should appear in the matrix exactly when something is rigged"
        );
    }

    #[test]
    fn manifest_lookup_covers_build_axis_manifests() {
        assert!(manifest("wasm").is_some());
        assert!(manifest("large-generated-clients").is_some());
        assert!(manifest("new-project").is_some());
        assert!(manifest("nope").is_none());
    }
}
