/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rustsdk.customize.dsql.DsqlDecorator
import software.amazon.smithy.rustsdk.customize.rds.RdsDecorator

/**
 * These inlineables have `#[cfg(test)]` modules using `#[tokio::test]`, so they need to declare tokio
 * as a dev-dependency. Otherwise tokio only shows up by luck: from protocol test codegen, or from
 * `IntegrationTestDependencies` when the service has a dir in `aws/sdk/integration-tests/`.
 *
 * The models below have neither, so without the dev-dependency `cargo test` fails to build while
 * `cargo build` succeeds.
 *
 * `endpoint_discovery` has the same gap, but generating it needs a `DescribeEndpoints` operation, so
 * it's verified against the real `aws-sdk-timestreamwrite` crate instead.
 */
internal class InlineableTestDependenciesTest {
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

                @httpHeader("x-amz-response-validation-mode")
                validationMode: ValidationMode

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

            enum ValidationMode {
                ENABLED
            }
            """.asSmithyModel(smithyVersion = "2")

        /** A model whose only operation opts into both request and response checksums. */
        private val checksumModel =
            model(
                """
                @httpChecksum(
                    requestChecksumRequired: true,
                    requestAlgorithmMember: "checksumAlgorithm",
                    requestValidationModeMember: "validationMode",
                    responseAlgorithms: ["CRC32"]
                )
                """,
            )

        private val plainModel = model()
    }

    /**
     * `http_request_checksum` refers to `crate::presigning::PresigningMarker`, which these models
     * don't wire up. Naming the type in a comment is enough to pull the inlineable in, and it needs
     * the `http-1x` feature alongside it.
     */
    private fun withPresigning(): (Any, RustCrate) -> Unit =
        { _, rustCrate ->
            rustCrate.mergeFeature(Feature("http-1x", default = false, listOf("aws-smithy-runtime-api/http-1x")))
            rustCrate.integrationTest("presigning_marker") {
                rustTemplate(
                    "//#{PresigningMarker};",
                    "PresigningMarker" to AwsRuntimeType.presigning().resolve("PresigningMarker"),
                )
            }
        }

    private fun assertCargoTestBuilds(
        model: Model,
        decorators: List<ClientCodegenDecorator> = listOf(),
        needsPresigning: Boolean = false,
    ) {
        awsSdkIntegrationTest(model, additionalDecorators = decorators) { ctx, rustCrate ->
            if (needsPresigning) {
                withPresigning()(ctx, rustCrate)
            }
        }
    }

    @Test
    fun httpRequestAndResponseChecksumInlineables() {
        assertCargoTestBuilds(checksumModel, needsPresigning = true)
    }

    @Test
    fun dsqlAuthTokenInlineable() {
        assertCargoTestBuilds(plainModel, decorators = listOf(DsqlDecorator()))
    }

    @Test
    fun rdsAuthTokenInlineable() {
        assertCargoTestBuilds(plainModel, decorators = listOf(RdsDecorator()))
    }
}
