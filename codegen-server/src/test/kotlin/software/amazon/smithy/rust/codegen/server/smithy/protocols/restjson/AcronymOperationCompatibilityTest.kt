/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.restjson

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Tag
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.customizations.ServerServiceSchemaGenerator
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

/**
 * Generates legacy and schema services with acronym-heavy operation names. It compiles and runs
 * both successful calls and modeled-error calls, checks descriptor references, and compares the
 * complete HTTP responses. This catches inconsistent case conversion between operation markers,
 * error tables, and schema descriptors.
 */
@Tag("server-compatibility")
internal class AcronymOperationCompatibilityTest {
    @Test
    fun `acronym operations compile and preserve legacy responses`() {
        val run = Files.createTempDirectory("acronym-operation-parity-")
        val model =
            """
            ${'$'}version: "2"
            namespace test.acronymoperations
            use aws.protocols#restJson1
            @restJson1 service Acronyms { version: "1", operations: [ACLsUpdateStatus, HTTP2GetStatus] }
            @http(method: "POST", uri: "/acls")
            operation ACLsUpdateStatus { input := {}, output := {}, errors: [Denied] }
            @http(method: "POST", uri: "/http2")
            operation HTTP2GetStatus { input := {}, output := {} }
            @error("client") @httpError(403) structure Denied { @required message: String }
            """.asSmithyModel()
        for (schemaSerde in listOf(false, true)) {
            val snapshot = run.resolve("$schemaSerde.txt")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ServerAdditionalSettings.builder().withHttp1x().schemaSerde(schemaSerde).toObjectNode(),
                    overrideTestDir =
                        run.resolve(
                            if (schemaSerde) {
                                "schema"
                            } else {
                                "legacy"
                            },
                        ).toFile(),
                    cargoCommand = "cargo test --quiet -- --nocapture",
                ),
                testCoverage = HttpTestType.Default,
            ) { context, crate ->
                val operations = context.model.operationShapes.associateBy { it.id.name }
                val deniedOperation = operations.getValue("ACLsUpdateStatus")
                val successOperation = operations.getValue("HTTP2GetStatus")
                val deniedSetter = context.symbolProvider.toSymbol(deniedOperation).name.toSnakeCase()
                val successSetter = context.symbolProvider.toSymbol(successOperation).name.toSnakeCase()
                crate.testModule {
                    tokioTest("acronym_operation_responses") {
                        rustTemplate(
                            """
                            // Preserve the public marker spellings observed in the generated serde service.
                            assert_eq!(
                                <crate::operation_shape::AcLsUpdateStatus as #{Server}::operation::OperationShape>::ID.absolute(),
                                "test.acronymoperations##ACLsUpdateStatus",
                            );
                            assert_eq!(
                                <crate::operation_shape::Http2GetStatus as #{Server}::operation::OperationShape>::ID.absolute(),
                                "test.acronymoperations##HTTP2GetStatus",
                            );
                            #{SchemaAssertions}
                            let app = crate::Acronyms::builder(crate::AcronymsConfig::builder().build())
                                .$deniedSetter(|_: #{DeniedInput}| async move {
                                    #{Err}::<#{DeniedOutput}, _>(#{OperationError}::from(crate::error::Denied {
                                        message: "denied".into(),
                                    }))
                                })
                                .$successSetter(|_: #{SuccessInput}| async move { #{SuccessOutput} {} })
                                .build().unwrap();
                            let mut records = #{String}::new();
                            for (path, expected_status, expected_body) in [
                                ("/acls", 403, "{\"message\":\"denied\"}"),
                                ("/http2", 200, "{}"),
                            ] {
                                let request = #{Http}::Request::post(path).header("content-type", "application/json")
                                    .body(#{Server}::body::to_boxed_sync("{}")).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(app.clone(), request).await.unwrap();
                                let status = response.status().as_u16();
                                let mut headers: #{Vec}<_> = response.headers().iter()
                                    .map(|(key, value)| (key.to_string(), value.to_str().unwrap().to_string())).collect();
                                headers.sort();
                                #{Collect}
                                let text = ::std::str::from_utf8(&body).unwrap();
                                println!("{path} {status} {headers:?} {text}");
                                assert_eq!(status, expected_status);
                                assert_eq!(text, expected_body);
                                records.push_str(&format!("{path} {status} {headers:?} {text}\n"));
                            }
                            ::std::fs::write(${snapshot.toString().dq()}, records).unwrap();
                            """,
                            *RuntimeType.preludeScope,
                            "DeniedInput" to context.symbolProvider.toSymbol(context.model.expectShape(deniedOperation.input.get())),
                            "DeniedOutput" to context.symbolProvider.toSymbol(context.model.expectShape(deniedOperation.output.get())),
                            "SuccessInput" to context.symbolProvider.toSymbol(context.model.expectShape(successOperation.input.get())),
                            "SuccessOutput" to context.symbolProvider.toSymbol(context.model.expectShape(successOperation.output.get())),
                            "OperationError" to context.symbolProvider.symbolForOperationError(deniedOperation),
                            "SchemaAssertions" to
                                writable {
                                    if (schemaSerde) {
                                        rustTemplate(
                                            """
                                            // Marker traits must reference the descriptor declared for that operation.
                                            assert!(::std::ptr::eq(
                                                <crate::operation_shape::AcLsUpdateStatus as #{Server}::operation::SchemaOperationShape>::SCHEMA,
                                                &crate::schema::operations::${ServerServiceSchemaGenerator.operationSchemaConstName(context.symbolProvider.toSymbol(deniedOperation))},
                                            ));
                                            assert!(::std::ptr::eq(
                                                <crate::operation_shape::Http2GetStatus as #{Server}::operation::SchemaOperationShape>::SCHEMA,
                                                &crate::schema::operations::${ServerServiceSchemaGenerator.operationSchemaConstName(context.symbolProvider.toSymbol(successOperation))},
                                            ));
                                            """,
                                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                                        )
                                    }
                                },
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            "Collect" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "body", "response"),
                        )
                    }
                }
            }
        }
        assertEquals(Files.readString(run.resolve("false.txt")), Files.readString(run.resolve("true.txt")), run.toString())
    }
}
