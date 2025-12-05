/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.MethodSource
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.io.File

/**
 * Tests for HTTP dependency selection based on http-1x flag.
 * Verifies that the correct dependencies are included in Cargo.toml and that code compiles.
 * Uses cargo_metadata to parse and validate dependencies instead of string matching.
 */
internal class Http1xDependencyTest {
    private val cargoMetadata = CargoDependency("cargo_metadata", CratesIo("0.18"), features = setOf("builder")).toType()
    private val semver = CargoDependency("semver", CratesIo("1.0")).toType()

    private fun buildAdditionalSettings(publicConstrainedTypes: Boolean): ObjectNode {
        val builder = ServerAdditionalSettings.builder()
        return builder
            .publicConstrainedTypes(publicConstrainedTypes)
            .generateCodegenComments(true)
            .toObjectNode()
    }

    private fun verify_http0x_dependencies(
        testName: String,
        additionalDeps: List<Triple<String, String, List<String>?>> = emptyList(),
    ) = writable {
        rustTemplate(
            """
            let metadata = #{MetadataCommand}::new()
                .exec()
                .expect("Failed to run cargo metadata");

            let root_package = metadata.root_package()
                .expect("Failed to get root package");

            // Check all HTTP 0.x dependencies have minimum versions
            let http0_crates = parse_crate_min_versions(&[
                ("http", "0.2.0", None),
                ("aws-smithy-legacy-http-server", "0.65.7", None),
                ("aws-smithy-legacy-http", "0.62.5", None),
                ("aws-smithy-runtime-api", "1.9.1", Some(&["http-02x"])),
            ]);

            verify_dependencies(&metadata, root_package, &http0_crates);

            // Verify http crate does NOT accept version 1.x
            let http_dep = root_package.dependencies.iter()
                .find(|dep| dep.name == "http")
                .expect("Should have http dependency");
            let http_req = #{VersionReq}::parse(&http_dep.req.to_string())
                .expect("Failed to parse http version requirement");
            let v1 = #{Version}::parse("1.0.0").unwrap();
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
            "MetadataCommand" to cargoMetadata.resolve("MetadataCommand"),
            "VersionReq" to semver.resolve("VersionReq"),
            "Version" to semver.resolve("Version"),
            *preludeScope,
        )
    }

    private fun define_util_functions() =
        writable {
            rustTemplate(
                """
            ##[cfg(test)]
            fn extract_min_version(req: &#{VersionReq}) -> #{Version} {
                // Handle wildcard (*) requirements - empty comparators means "any version"
                if req.comparators.is_empty() {
                    return #{Version}::new(0, 0, 0);
                }

                req.comparators.iter()
                    .filter_map(|c| {
                        match c.op {
                            #{Op}::GreaterEq | #{Op}::Exact | #{Op}::Caret | #{Op}::Tilde => {
                                Some(#{Version} {
                                    major: c.major,
                                    minor: c.minor.unwrap_or(0),
                                    patch: c.patch.unwrap_or(0),
                                    pre: #{Prerelease}::EMPTY,
                                    build: #{BuildMetadata}::EMPTY,
                                })
                            }
                            _ => None,
                        }
                    })
                    .min()
                    .expect("Could not determine minimum version from requirement")
            }

            ##[cfg(test)]
            fn get_package_version(metadata: &#{Metadata}, package_name: &str) -> #{Version} {
                // Find the package in the metadata (works for both path and registry dependencies)
                metadata.packages.iter()
                    .find(|pkg| pkg.name == package_name)
                    .map(|pkg| #{Version}::parse(&pkg.version.to_string()).expect("Failed to parse package version"))
                    .expect(&format!("Could not find package {} in metadata", package_name))
            }

            ##[cfg(test)]
            fn parse_crate_min_versions<'a>(
                crates: &[(&'a str, &str, #{Option}<&[&str]>)]
            ) -> #{Vec}<(&'a str, #{Version}, #{Option}<#{Vec}<#{String}>>)> {
                crates.iter()
                    .map(|(name, ver_str, features_opt)| {
                        let version = #{Version}::parse(ver_str)
                            .expect(&format!("Invalid version string '{}' for {}", ver_str, name));
                        let features = features_opt.map(|f| {
                            f.iter().map(|s| s.to_string()).collect::<#{Vec}<#{String}>>()
                        });
                        (*name, version, features)
                    })
                    .collect()
            }

            ##[cfg(test)]
            fn satisfies_minimum_version(actual: &#{Dependency}, expected_min: &#{Version}, metadata: &#{Metadata}) -> bool {
                let actual_version = if actual.path.is_some() || actual.req.to_string() == "*" {
                    // For path dependencies, get actual version from package metadata
                    get_package_version(metadata, &actual.name)
                } else {
                    // For registry dependencies, extract minimum version from requirement
                    extract_min_version(&actual.req)
                };

                actual_version >= *expected_min
            }

            ##[allow(dead_code)]
            ##[cfg(test)]
            fn has_required_features(actual: &#{Dependency}, required: &[#{String}]) -> bool {
                required.iter().all(|f| actual.features.contains(f))
            }

            ##[cfg(test)]
            fn verify_dependencies(
                metadata: &#{Metadata},
                root_package: &#{Package},
                expected: &[(&str, #{Version}, #{Option}<#{Vec}<#{String}>>)]
            ) {
                for (dep_name, expected_min, expected_features) in expected {
                    let dep = root_package.dependencies.iter()
                        .find(|d| d.name == *dep_name)
                        .expect(&format!("Must have `{}` dependency", dep_name));

                    // Check version
                    assert!(
                        satisfies_minimum_version(dep, expected_min, metadata),
                        "{} does not satisfy minimum version >= {}. Actual requirement: {} (path: {:?})",
                        dep_name, expected_min, dep.req, dep.path
                    );

                    // Check features if specified
                    if let #{Some}(required_features) = expected_features {
                        assert!(
                            has_required_features(dep, required_features),
                            "{} does not have required features: {:?}. Actual features: {:?}",
                            dep_name, required_features, dep.features
                        );
                    }
                }
            }
            """,
                "VersionReq" to semver.resolve("VersionReq"),
                "Version" to semver.resolve("Version"),
                "Op" to semver.resolve("Op"),
                "Prerelease" to semver.resolve("Prerelease"),
                "BuildMetadata" to semver.resolve("BuildMetadata"),
                "Metadata" to cargoMetadata.resolve("Metadata"),
                "Package" to cargoMetadata.resolve("Package"),
                "Dependency" to cargoMetadata.resolve("Dependency"),
                *preludeScope,
            )
        }

    @ParameterizedTest
    @MethodSource("protocolAndConstrainedTypesProvider")
    fun `SDK with http-1x enabled compiles and has correct dependencies`(
        protocol: ModelProtocol,
        publicConstrainedTypes: Boolean,
    ) {
        val (model) = loadSmithyConstraintsModelForProtocol(protocol)
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(publicConstrainedTypes),
                cargoCommand = "cargo test --all-features",
            ),
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { _, rustCrate ->
            rustCrate.lib {
                define_util_functions().invoke(this)

                unitTest("http_1x_dependencies") {
                    rustTemplate(
                        """
                        let metadata = #{MetadataCommand}::new()
                            .exec()
                            .expect("Failed to run cargo metadata");

                        let root_package = metadata.root_package()
                            .expect("Failed to get root package");

                        // Check all HTTP 1.x dependencies have minimum versions and features
                        let http1_crates = parse_crate_min_versions(&[
                            ("http", "1.0.0", None),
                            ("aws-smithy-http", "0.63.0", None),
                            ("aws-smithy-http-server", "0.66.0", None),
                            ("http-body-util", "0.1.3", None),
                            ("aws-smithy-types", "1.3.3", Some(&["http-body-1-x"])),
                            ("aws-smithy-runtime-api", "1.9.1", Some(&["http-1x"])),
                        ]);

                        verify_dependencies(&metadata, root_package, &http1_crates);

                        // Should NOT have legacy dependencies
                        let legacy = ["aws-smithy-http-legacy-server", "aws-smithy-legacy-http"];
                        for legacy_crate in legacy {
                            assert!(
                                !root_package.dependencies.iter().any(|dep| dep.name == legacy_crate),
                                "Should NOT have {legacy_crate} dependency"
                            );
                        }
                        """,
                        "MetadataCommand" to cargoMetadata.resolve("MetadataCommand"),
                        *preludeScope,
                    )
                }
            }
        }
    }

    @ParameterizedTest
    @MethodSource("protocolAndConstrainedTypesProvider")
    fun `SDK defaults to http-1x disabled, and dependencies are correct`(protocol: ModelProtocol) {
        val (model) = loadSmithyConstraintsModelForProtocol(protocol)
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(publicConstrainedTypes = false),
                cargoCommand = "cargo test --all-features",
            ),
            testCoverage = HttpTestType.AsConfigured,
        ) { _, rustCrate ->
            rustCrate.lib {
                define_util_functions().invoke(this)

                unitTest("http_0x_dependencies") {
                    verify_http0x_dependencies("http_0x_dependencies").invoke(this)
                }
            }
        }
    }

    @Test
    fun `SDK with http-1x enabled for rpcv2Cbor extras model has correct dependencies`() {
        val model = loadRpcv2CborExtrasModel()
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(publicConstrainedTypes = false),
                cargoCommand = "cargo test --all-features",
            ),
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { _, rustCrate ->
            rustCrate.lib {
                define_util_functions().invoke(this)

                unitTest("http_1x_dependencies_rpcv2cbor_extras") {
                    rustTemplate(
                        """
                        let metadata = #{MetadataCommand}::new()
                            .exec()
                            .expect("Failed to run cargo metadata");

                        let root_package = metadata.root_package()
                            .expect("Failed to get root package");

                        // Check all HTTP 1.x dependencies have minimum versions and features
                        let http1_crates = parse_crate_min_versions(&[
                            ("http", "1.0.0", None),
                            ("aws-smithy-http", "0.63.0", None),
                            ("aws-smithy-http-server", "0.66.0", None),
                            ("http-body-util", "0.1.3", None),
                            ("aws-smithy-types", "1.3.3", Some(&["http-body-1-x"])),
                            ("aws-smithy-runtime-api", "1.9.1", Some(&["http-1x"])),
                        ]);

                        verify_dependencies(&metadata, root_package, &http1_crates);

                        // Should NOT have legacy dependencies
                        let legacy = ["aws-smithy-http-legacy-server", "aws-smithy-legacy-http"];
                        for legacy_crate in legacy {
                            assert!(
                                !root_package.dependencies.iter().any(|dep| dep.name == legacy_crate),
                                "Should NOT have {legacy_crate} dependency"
                            );
                        }
                        """,
                        "MetadataCommand" to cargoMetadata.resolve("MetadataCommand"),
                        *preludeScope,
                    )
                }
            }
        }
    }

    @Test
    fun `SDK defaults to http-0x for rpcv2Cbor extras model and dependencies are correct`() {
        val model = loadRpcv2CborExtrasModel()
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(publicConstrainedTypes = false),
                cargoCommand = "cargo test --all-features",
            ),
            testCoverage = HttpTestType.AsConfigured,
        ) { _, rustCrate ->
            rustCrate.lib {
                define_util_functions().invoke(this)

                unitTest("http_0x_dependencies_rpcv2cbor_extras") {
                    rustTemplate(
                        """
                        let metadata = #{MetadataCommand}::new()
                            .exec()
                            .expect("Failed to run cargo metadata");

                        let root_package = metadata.root_package()
                            .expect("Failed to get root package");

                        // Check all HTTP 0.x dependencies have minimum versions
                        let http0_crates = parse_crate_min_versions(&[
                            ("http", "0.2.0", None),
                            ("aws-smithy-legacy-http-server", "0.65.7", None),
                            ("aws-smithy-legacy-http", "0.62.5", Some(&["event-stream"])),
                            ("aws-smithy-runtime-api", "1.9.1", Some(&["http-02x"])),
                        ]);

                        verify_dependencies(&metadata, root_package, &http0_crates);

                        // Verify http crate does NOT accept version 1.x
                        let http_dep = root_package.dependencies.iter()
                            .find(|dep| dep.name == "http")
                            .expect("Should have http dependency");
                        let http_req = #{VersionReq}::parse(&http_dep.req.to_string())
                            .expect("Failed to parse http version requirement");
                        let v1 = #{Version}::parse("1.0.0").unwrap();
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
                        "MetadataCommand" to cargoMetadata.resolve("MetadataCommand"),
                        "VersionReq" to semver.resolve("VersionReq"),
                        "Version" to semver.resolve("Version"),
                        *preludeScope,
                    )
                }
            }
        }
    }

    @Test
    fun `SDK with explicit http-1x disabled has correct dependencies`() {
        val (model) = loadSmithyConstraintsModelForProtocol(ModelProtocol.RestJson)
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(publicConstrainedTypes = false),
                cargoCommand = "cargo test --all-features",
            ),
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_0_X),
        ) { _, rustCrate ->
            rustCrate.lib {
                define_util_functions().invoke(this)

                unitTest("explicit_http_0x_disabled") {
                    verify_http0x_dependencies("explicit_http_0x_disabled").invoke(this)
                }
            }
        }
    }

    companion object {
        @JvmStatic
        fun protocolAndConstrainedTypesProvider(): List<Arguments> {
            // Constraints are not implemented for RestXML protocol.
            val protocols =
                ModelProtocol.entries.filter {
                    !it.name.matches(Regex("RestXml.*"))
                }
            val constrainedSettings = listOf(true, false)

            return protocols.flatMap { protocol ->
                constrainedSettings.map { publicConstrained ->
                    Arguments.of(protocol, publicConstrained)
                }
            }
        }
    }
}

/**
 * Loads the rpcv2Cbor-extras model defined in the common repository and returns the model.
 */
fun loadRpcv2CborExtrasModel(): Model {
    val filePath = "../codegen-core/common-test-models/rpcv2Cbor-extras.smithy"
    return File(filePath).readText().asSmithyModel()
}
