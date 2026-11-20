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
 * The generated `aws-lc-fips` feature fans out to three runtime crates, and its checksums arm has
 * to be scoped: `dep/feat` requires `dep` to be a declared dependency, so naming
 * `aws-smithy-checksums/aws-lc-rs-fips` unconditionally would produce an invalid manifest for
 * every service crate that has no checksum operations.
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

        /**
         * Generates [checksumModel] and returns its `Cargo.toml`.
         *
         * The `http_request_checksum` inlineable refers to `crate::presigning`, whose deprecated
         * http 0.2.x conversions are `#[cfg]`-gated on `http-02x`, and `AwsPresigningDecorator`
         * declares that feature only for models with presignable shapes. See
         * `InlineableTestDependenciesTest`. The plain model must *not* get these: with no
         * presignable shape it has no `http` dependency for `dep:http` to name.
         */
        private fun checksumCargoToml() =
            awsSdkIntegrationTest(checksumModel) { _, rustCrate ->
                rustCrate.mergeFeature(Feature("http-1x", default = false, listOf("aws-smithy-runtime-api/http-1x")))
                rustCrate.mergeFeature(
                    Feature("http-02x", default = false, listOf("dep:http", "aws-smithy-runtime-api/http-02x")),
                )
            }.resolve("Cargo.toml").readText()

        /** Generates [plainModel] and returns its `Cargo.toml`. */
        private fun plainCargoToml() = awsSdkIntegrationTest(plainModel).resolve("Cargo.toml").readText()
    }

    @Test
    fun `aws-lc-fips feature covers tls signing and checksums for a service with checksum operations`() {
        val cargoToml = checksumCargoToml()

        assert(
            cargoToml.contains(
                """aws-lc-fips = ["aws-smithy-runtime/aws-lc-fips", "aws-runtime/aws-lc-fips", "aws-smithy-checksums/aws-lc-rs-fips"]""",
            ),
        ) {
            "Expected an `aws-lc-fips` feature fanning out to all three crypto paths.\n$cargoToml"
        }
    }

    @Test
    fun `aws-lc-fips feature omits checksums for a service without checksum operations`() {
        val cargoToml = plainCargoToml()

        assert(cargoToml.contains("""aws-lc-fips = ["aws-smithy-runtime/aws-lc-fips", "aws-runtime/aws-lc-fips"]""")) {
            "Expected an `aws-lc-fips` feature covering TLS and signing only.\n$cargoToml"
        }
        // The arm is scoped by whether the crate is a dependency at all, so the crate's absence is
        // what makes omitting it correct rather than a missed case.
        assert(!cargoToml.contains("aws-smithy-checksums")) {
            "A service with no checksum operations must not reference aws-smithy-checksums.\n$cargoToml"
        }
    }

    @Test
    fun `aws-lc-fips is not a default feature`() {
        val cargoToml = plainCargoToml()

        // The validated AWS-LC build is unavailable on some targets the SDK supports and needs
        // CMake and Go at build time, so it can only ever be opt-in.
        val defaultFeatures = cargoToml.lines().first { it.startsWith("default = [") }
        assert(!defaultFeatures.contains("\"aws-lc-fips\"")) {
            "`aws-lc-fips` must not be a default feature, but default was: $defaultFeatures"
        }
    }

    /**
     * The `rustcrypto` counterpart to `aws-lc-fips`, asserted on the manifest because neither way
     * of breaking it shows up as a behavioural failure: drop the generated `rustcrypto` feature
     * and an ordinary build loses its backend while the FIPS build still succeeds; drop
     * `default-features = false` and both succeed while the FIPS build quietly compiles RustCrypto
     * again. A service crate is the last link in the chain that forwards the choice.
     */
    @Test
    fun `rustcrypto feature covers signing and checksums for a service with checksum operations`() {
        val cargoToml = checksumCargoToml()

        // Signing is reached through `aws-runtime` rather than named directly, because that is the
        // crate that declares `aws-sigv4` with its defaults off.
        assert(
            cargoToml.contains("""rustcrypto = ["aws-runtime/rustcrypto", "aws-smithy-checksums/rustcrypto"]"""),
        ) {
            "Expected a `rustcrypto` feature forwarding to both backends.\n$cargoToml"
        }
    }

    @Test
    fun `rustcrypto feature omits checksums for a service without checksum operations`() {
        val cargoToml = plainCargoToml()

        assert(cargoToml.contains("""rustcrypto = ["aws-runtime/rustcrypto"]""")) {
            "Expected a `rustcrypto` feature covering signing only.\n$cargoToml"
        }
    }

    @Test
    fun `a generated crate names a crypto backend in its default features`() {
        // Both shapes of service, because the two fan out differently and only one of them names
        // the checksums backend.
        val manifests =
            listOf(
                "with checksum operations" to checksumCargoToml(),
                "without checksum operations" to plainCargoToml(),
            )

        for ((name, cargoToml) in manifests) {
            // Without this the crate has no crypto at all on a plain `cargo add`, since the
            // backends it depends on are declared with their defaults off. RustCrypto is the one
            // that can be a default: it is pure Rust and builds everywhere the SDK does.
            val defaultFeatures = cargoToml.lines().first { it.startsWith("default = [") }
            assert(defaultFeatures.contains("\"rustcrypto\"")) {
                "A generated crate ($name) must default to a crypto backend, but default was: $defaultFeatures"
            }
        }
    }

    @Test
    fun `crypto backend crates are declared with their default features off`() {
        val cargoToml = checksumCargoToml()

        // `aws-runtime` and `aws-sigv4` carry the signing backend, `aws-smithy-checksums` the
        // digest backend. Each has to be declared with `default-features = false` for the
        // `aws-lc-fips` feature to be a substitution rather than an addition.
        for (dependency in listOf("aws-runtime", "aws-sigv4", "aws-smithy-checksums")) {
            val declaration = dependencyDeclaration(cargoToml, dependency)
            assert(declaration != null) {
                "Expected the generated crate to declare `$dependency`.\n$cargoToml"
            }
            assert(declaration!!.contains("default-features = false")) {
                "`$dependency` must be declared with `default-features = false`, otherwise an " +
                    "`aws-lc-fips` build still compiles the RustCrypto backend. Declaration was:\n$declaration"
            }
        }
    }

    /**
     * Returns the body of a `[dependencies.<name>]` table, or null if the crate is not declared.
     * Reads the table form because that is what codegen emits; the published manifests are the
     * same tables after `fix-manifests` adds version requirements.
     */
    private fun dependencyDeclaration(
        cargoToml: String,
        name: String,
    ): String? {
        val lines = cargoToml.lines()
        val start = lines.indexOfFirst { it.trim() == "[dependencies.$name]" }
        if (start < 0) {
            return null
        }
        val body = lines.drop(start + 1).takeWhile { !it.trimStart().startsWith("[") }
        return body.joinToString("\n")
    }
}
