/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import kotlin.io.path.readText

/**
 * The generated `fips` feature is the single switch that routes a service crate's cryptography
 * through the FIPS 140-3 validated build of AWS-LC. It fans out to three runtime crates, and the
 * checksums arm has to be scoped: naming `aws-smithy-checksums/fips` unconditionally would make
 * Cargo add that crate to every service crate, including the ones that never otherwise use it.
 */
internal class FipsFeatureTest {
    companion object {
        private fun model(operations: String = "") =
            """
            namespace test
            use aws.api#service
            use aws.auth#sigv4
            use aws.protocols#httpChecksum
            use aws.protocols#restJson1
            use smithy.rules#endpointRuleSet

            @service(sdkId: "dontcare")
            @restJson1
            @sigv4(name: "dontcare")
            @auth([sigv4])
            @endpointRuleSet({
                "version": "1.0",
                "rules": [{ "type": "endpoint", "conditions": [], "endpoint": { "url": "https://example.com" } }],
                "parameters": {
                    "Region": { "required": false, "type": "String", "builtIn": "AWS::Region" },
                }
            })
            service TestService {
                version: "2023-01-01",
                operations: [SomeOperation]
            }

            @http(uri: "/SomeOperation", method: "POST")
            @optionalAuth
            $operations
            operation SomeOperation {
                input: SomeInput,
                output: SomeOutput
            }

            @input
            structure SomeInput {
                @httpHeader("x-amz-request-algorithm")
                checksumAlgorithm: ChecksumAlgorithm

                @httpHeader("x-amz-checksum-crc32")
                ChecksumCRC32: String

                @httpPayload
                @required
                body: Blob
            }

            @output
            structure SomeOutput {}

            enum ChecksumAlgorithm {
                CRC32
            }
            """.asSmithyModel(smithyVersion = "2")

        /** A service whose operation carries `@httpChecksum`, so it depends on `aws-smithy-checksums`. */
        private val checksumModel =
            model(
                """
                @httpChecksum(
                    requestChecksumRequired: true,
                    requestAlgorithmMember: "checksumAlgorithm",
                )
                """,
            )

        /** A service with no checksum operations, which never depends on `aws-smithy-checksums`. */
        private val plainModel = model()
    }

    @Test
    fun `fips feature covers tls signing and checksums for a service with checksum operations`() {
        // The `http_request_checksum` inlineable refers to `crate::presigning`, which is `#[cfg]`-gated on
        // `http-02x`. `AwsPresigningDecorator` declares that feature only for models with presignable shapes,
        // so declare it here to keep the generated crate compiling. See `InlineableTestDependenciesTest`.
        val cargoToml =
            awsSdkIntegrationTest(checksumModel) { _, rustCrate ->
                rustCrate.mergeFeature(Feature("http-1x", default = false, listOf("aws-smithy-runtime-api/http-1x")))
                rustCrate.mergeFeature(
                    Feature("http-02x", default = false, listOf("dep:http", "aws-smithy-runtime-api/http-02x")),
                )
            }.resolve("Cargo.toml").readText()

        assert(
            cargoToml.contains(
                """fips = ["aws-smithy-runtime/crypto-fips", "aws-runtime/fips", "aws-smithy-checksums/fips"]""",
            ),
        ) {
            "Expected a `fips` feature fanning out to all three crypto paths.\n$cargoToml"
        }
    }

    @Test
    fun `fips feature omits checksums for a service without checksum operations`() {
        val cargoToml = awsSdkIntegrationTest(plainModel).resolve("Cargo.toml").readText()

        assert(cargoToml.contains("""fips = ["aws-smithy-runtime/crypto-fips", "aws-runtime/fips"]""")) {
            "Expected a `fips` feature covering TLS and signing only.\n$cargoToml"
        }
        // The arm is scoped by whether the crate is a dependency at all, so the crate's absence is
        // what makes omitting it correct rather than a missed case.
        assert(!cargoToml.contains("aws-smithy-checksums")) {
            "A service with no checksum operations must not reference aws-smithy-checksums.\n$cargoToml"
        }
    }

    @Test
    fun `fips is not a default feature`() {
        val cargoToml = awsSdkIntegrationTest(plainModel).resolve("Cargo.toml").readText()

        // The validated AWS-LC build is unavailable on some targets the SDK supports and needs
        // CMake and Go at build time, so it can only ever be opt-in.
        val defaultFeatures = cargoToml.lines().first { it.startsWith("default = [") }
        assert(!defaultFeatures.contains("\"fips\"")) {
            "`fips` must not be a default feature, but default was: $defaultFeatures"
        }
    }
}
