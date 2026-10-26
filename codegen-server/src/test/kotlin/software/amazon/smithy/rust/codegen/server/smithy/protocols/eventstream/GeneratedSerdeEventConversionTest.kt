/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols.eventstream

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

/**
 * Generates legacy HTTP 1.x event-stream types for REST JSON, AWS JSON 1.0/1.1, and rpcv2Cbor.
 * Rust assertions construct unconstrained event variants with optional header, scalar, or nested
 * payload members absent and convert them to modeled events with TryInto. Every conversion must
 * succeed, including a nested structure containing an enum whose conversion can otherwise fail.
 * These assertions exercise generated conversion code; event-frame decoding has separate tests.
 */
internal class GeneratedSerdeEventConversionTest {
    @ParameterizedTest
    @ValueSource(strings = ["restJson1", "awsJson1_0", "awsJson1_1", "rpcv2Cbor"])
    fun `generated serde event conversions accept absent optional members`(protocol: String) {
        val model =
            """
            ${'$'}version: "2"
            namespace test.eventfallibility
            use ${if (protocol == "rpcv2Cbor") "smithy.protocols" else "aws.protocols"}#$protocol
            @$protocol service Events { version: "1", operations: [Listen] }
            @http(method: "POST", uri: "/listen")
            operation Listen { input := { @required @httpPayload events: EventStream }, output := {} }
            @streaming union EventStream {
                empty: Empty
                scalar: Scalar
                structured: Structured
                enumBody: EnumBody
            }
            structure Empty {}
            structure Scalar { @eventHeader header: String, value: String }
            structure Structured { value: Nested }
            structure Nested { value: String }
            structure EnumBody { value: NestedEnum }
            structure NestedEnum { kind: Kind }
            enum Kind {
                VALID = "valid"
            }
            """.asSmithyModel()
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                // Keep generated serde enabled for every protocol.
                additionalSettings = ServerAdditionalSettings.builder().schemaSerde(false).withHttp1x().toObjectNode(),
                overrideTestDir = Files.createTempDirectory("generated-serde-event-conversion-$protocol-").toFile(),
                cargoCommand = "cargo test --quiet --lib",
            ),
            testCoverage = HttpTestType.Default,
        ) { _, crate ->
            crate.testModule {
                unitTest("optional_event_members_accept_absent_values") {
                    rustTemplate(
                        """
                        use crate::unconstrained::event_stream_unconstrained::EventStreamUnconstrained;
                        // Omit every optional member, including the whole enum-containing structure.
                        for event in [
                            EventStreamUnconstrained::Empty(crate::model::Empty {}),
                            EventStreamUnconstrained::Scalar(crate::model::Scalar { header: #{None}, value: #{None} }),
                            EventStreamUnconstrained::Structured(crate::model::Structured { value: #{None} }),
                            EventStreamUnconstrained::EnumBody(#{Default}::default()),
                        ] {
                            let completed: #{Result}<crate::model::EventStream, _> = event.try_into();
                            assert!(completed.is_ok());
                        }
                        """,
                        *RuntimeType.preludeScope,
                    )
                }
            }
        }
    }
}
