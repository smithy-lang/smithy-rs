/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * Tests for HTTP dependency selection based on http-1x flag.
 * Verifies that the correct dependencies are included in Cargo.toml and that code compiles.
 * Uses cargo_metadata to parse and validate dependencies instead of string matching.
 */
internal class Http1xDependencyTest {
    private val cargoMetadata = CargoDependency("cargo_metadata", CratesIo("0.18"))
    private val semver = CargoDependency("semver", CratesIo("1.0"))

    private val testModel =
        """
        namespace test
        use aws.protocols#restJson1

        @restJson1
        service TestService {
            version: "2024-03-18"
            operations: [GetStatus]
        }
        @http(uri: "/status", method: "GET")
        operation GetStatus {
            output: GetStatusOutput
        }
        structure GetStatusOutput {
            status: String
        }
    """.asSmithyModel()

    private fun buildAdditionalSettings(http1x: Boolean) =
        ServerAdditionalSettings.builder()
            .withHttp1x(http1x)
            .generateCodegenComments(true)
            .toObjectNode()

    private fun extract_min_version() = writable {
        rustTemplate(
           """
            ##[cfg(test)]
            fn extract_min_version(req: &#{VersionReq}) -> #{Version} {
                use semver::Op;

                // Handle wildcard (*) requirements - empty comparators means "any version"
                if req.comparators.is_empty() {
                    return #{Version}::new(0, 0, 0);
                }

                req.comparators.iter()
                    .filter_map(|c| {
                        match c.op {
                            Op::GreaterEq | Op::Exact | Op::Caret | Op::Tilde => {
                                Some(semver::Version {
                                    major: c.major,
                                    minor: c.minor.unwrap_or(0),
                                    patch: c.patch.unwrap_or(0),
                                    pre: semver::Prerelease::EMPTY,
                                    build: semver::BuildMetadata::EMPTY,
                                })
                            }
                            _ => None,
                        }
                    })
                    .min()
                    .expect("Could not determine minimum version from requirement")
            }

            ##[cfg(test)]
            fn get_package_version(metadata: &cargo_metadata::Metadata, package_name: &str) -> #{Version} {
                // Find the package in the metadata (works for both path and registry dependencies)
                metadata.packages.iter()
                    .find(|pkg| pkg.name == package_name)
                    .map(|pkg| #{Version}::parse(&pkg.version.to_string()).expect("Failed to parse package version"))
                    .expect(&format!("Could not find package {} in metadata", package_name))
            }
            """,
            "VersionReq" to semver.toType().resolve("VersionReq"),
            "Version" to semver.toType().resolve("Version"),
        )
    }

    @Test
    fun `SDK with http-1x enabled compiles and has correct dependencies`() {
        val (model, serviceShapeId) = loadSmithyConstraintsModelForProtocol(ModelProtocol.RestJson)
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(true),
                cargoCommand = "cargo test --all-features"
            ),
        ) { _, rustCrate ->
            rustCrate.lib {
                addDependency(cargoMetadata)

                extract_min_version().invoke(this)

                unitTest(
                    "http_1x_dependencies",
                    """
                    use semver::{Version, VersionReq};

                    let metadata = cargo_metadata::MetadataCommand::new()
                        .exec()
                        .expect("Failed to run cargo metadata");

                    let root_package = metadata.root_package()
                        .expect("Failed to get root package");

                    // Check all HTTP 1.x dependencies have minimum versions
                    let http1_crates = [
                        ("http", "1.0.0"),
                        ("aws-smithy-http", "0.63.0"),
                        ("aws-smithy-http-server", "0.66.0"),
                        ("http-body-util", "0.1.3"),
                    ];
                    for (dep_name, min_version_str) in http1_crates {
                        let dep = root_package.dependencies.iter()
                            .find(|d| d.name == dep_name)
                            .expect(&format!("Must have `{}` dependency", dep_name));

                        let expected_min = Version::parse(min_version_str)
                            .expect(&format!("Failed to parse expected min version for {}", dep_name));

                        // Check if this is a path dependency (wildcard requirement)
                        let actual_version = if dep.path.is_some() || dep.req.to_string() == "*" {
                            // For path dependencies, get the actual version from the package metadata
                            eprintln!("Info: {} is a path dependency, reading version from package metadata", dep_name);
                            get_package_version(&metadata, dep_name)
                        } else {
                            // For registry dependencies, extract minimum from the requirement
                            let req = VersionReq::parse(&dep.req.to_string())
                                .expect(&format!("Failed to parse version req for {}", dep_name));
                            extract_min_version(&req)
                        };

                        let source_msg = if dep.path.is_some() {
                            "path dependency".to_string()
                        } else {
                            format!("requirement: {}", dep.req)
                        };
                        assert!(
                            actual_version >= expected_min,
                            "{} version should be >= {}, but got: {} (from {})",
                            dep_name, expected_min, actual_version, source_msg
                        );
                    }

                    // Should NOT have legacy dependencies
                    let legacy = ["aws-smithy-http-legacy-server", "aws-smithy-legacy-http"];
                    for legacy_crate in legacy {
                        assert!(
                            !root_package.dependencies.iter().any(|dep| dep.name == legacy_crate),
                            "Should NOT have {legacy_crate} dependency"
                        );
                    }
                   """,
                )
            }
        }
    }

    @Test
    fun `SDK with http-1x disabled compiles and has correct dependencies`() {
        serverIntegrationTest(
            testModel,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(http1x = false),
            ),
        ) { _, rustCrate ->
            rustCrate.lib {
                addDependency(cargoMetadata)
                addDependency(semver)

                // Add helper function for extracting minimum version from requirements
                rust(
                    """
                    #[cfg(test)]
                    fn extract_min_version(req: &semver::VersionReq) -> semver::Version {
                        use semver::Op;
                        req.comparators.iter()
                            .filter_map(|c| {
                                match c.op {
                                    Op::GreaterEq | Op::Exact | Op::Caret | Op::Tilde => {
                                        Some(semver::Version {
                                            major: c.major,
                                            minor: c.minor.unwrap_or(0),
                                            patch: c.patch.unwrap_or(0),
                                            pre: semver::Prerelease::EMPTY,
                                            build: semver::BuildMetadata::EMPTY,
                                        })
                                    }
                                    _ => None,
                                }
                            })
                            .min()
                            .expect("Could not determine minimum version from requirement")
                    }
                    """,
                )

                unitTest(
                    "http_0x_dependencies",
                    """
                    use semver::{Version, VersionReq};

                    let metadata = cargo_metadata::MetadataCommand::new()
                        .exec()
                        .expect("Failed to run cargo metadata");

                    let root_package = metadata.root_package()
                        .expect("Failed to get root package");

                    // Check all HTTP 0.x dependencies have minimum versions
                    let http0_crates = [
                        ("http", "0.2.0"),
                        ("aws-smithy-http-legacy-server", "0.60.0"),
                    ];
                    for (dep_name, min_version_str) in http0_crates {
                        let dep = root_package.dependencies.iter()
                            .find(|d| d.name == dep_name)
                            .expect(&format!("Must have `{}` dependency", dep_name));

                        let expected_min = Version::parse(min_version_str)
                            .expect(&format!("Failed to parse expected min version for {}", dep_name));

                        // Check if this is a path dependency (wildcard requirement)
                        let actual_version = if dep.path.is_some() || dep.req.to_string() == "*" {
                            // For path dependencies, get the actual version from the package metadata
                            eprintln!("Info: {} is a path dependency, reading version from package metadata", dep_name);
                            get_package_version(&metadata, dep_name)
                        } else {
                            // For registry dependencies, extract minimum from the requirement
                            let req = VersionReq::parse(&dep.req.to_string())
                                .expect(&format!("Failed to parse version req for {}", dep_name));
                            extract_min_version(&req)
                        };

                        let source_msg = if dep.path.is_some() {
                            "path dependency".to_string()
                        } else {
                            format!("requirement: {}", dep.req)
                        };
                        assert!(
                            actual_version >= expected_min,
                            "{} version should be >= {}, but got: {} (from {})",
                            dep_name, expected_min, actual_version, source_msg
                        );
                    }

                    // Verify http crate does NOT accept version 1.x
                    let http_dep = root_package.dependencies.iter()
                        .find(|dep| dep.name == "http")
                        .expect("Should have http dependency");
                    let http_req = VersionReq::parse(&http_dep.req.to_string())
                        .expect("Failed to parse http version requirement");
                    let v1 = Version::parse("1.0.0").unwrap();
                    assert!(
                        !http_req.matches(&v1),
                        "http dependency should NOT accept version 1.x (must be < 1.0), but requirement is: {}", http_dep.req
                    );

                    // Should NOT have HTTP 1.x specific dependencies
                    let http1_only_crates = ["http-body-util", "aws-smithy-http", "aws-smithy-http-server"];
                    for dep_name in http1_only_crates {
                        assert!(
                            !root_package.dependencies.iter().any(|d| d.name == dep_name),
                            "Should NOT have `{}` dependency for HTTP 0.x", dep_name
                        );
                    }
                   """,
                )
            }
        }
    }

    @Test
    fun `SDK defaults to http-0x when no flag is provided`() {
        serverIntegrationTest(
            testModel,
            IntegrationTestParams(),
        ) { _, rustCrate ->
            rustCrate.lib {
                addDependency(cargoMetadata)
                addDependency(semver)
                unitTest(
                    "default_http_0x_dependencies",
                    """
                    use semver::{Version, VersionReq};

                    let metadata = cargo_metadata::MetadataCommand::new()
                        .exec()
                        .expect("Failed to run cargo metadata");

                    let root_package = metadata.root_package()
                        .expect("Failed to get root package");

                    // Should default to HTTP 0.x
                    let http_dep = root_package.dependencies.iter()
                        .find(|dep| dep.name == "http")
                        .expect("Should have http dependency");

                    let req = VersionReq::parse(&http_dep.req.to_string())
                        .expect("Failed to parse version requirement");

                    // Verify it uses HTTP 0.2.x by default
                    let v0_2 = Version::parse("0.2.0").unwrap();
                    assert!(
                        req.matches(&v0_2),
                        "Should default to http 0.2.x, got: {}", http_dep.req
                    );

                    assert!(
                        root_package.dependencies.iter().any(|dep| dep.name == "aws-smithy-http-legacy-server"),
                        "Should default to aws-smithy-http-legacy-server dependency"
                    );

                    assert!(
                        !root_package.dependencies.iter().any(|dep| dep.name == "http-body-util"),
                        "Should NOT have http-body-util dependency by default"
                    );
                    """,
                )
            }
        }
    }
}
