/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols.rpcv2

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * Generates rpcv2Cbor modeled errors with members carrying HTTP header bindings. With schemaSerde
 * omitted or disabled, the error serializer must compile and retain those members in the CBOR
 * document rather than writing HTTP headers. The schema case checks the same protocol rule,
 * including the modeled error status and encoded member values.
 */
internal class LegacyCborErrorHeadersCompatibilityTest {
    private val model =
        """
        ${'$'}version: "2"
        namespace test.cborheaders
        use smithy.protocols#rpcv2Cbor
        @rpcv2Cbor
        service Example { version: "1", operations: [Write] }
        operation Write {
            input := {}
            output := {}
            errors: [Failure]
        }
        @error("client")
        structure Failure {
            message: String
            @httpHeader("X-Error") detail: String
        }
        """.asSmithyModel()

    @Test
    fun `legacy cbor errors compile and keep header members in the document`() {
        // Fresh mainline at 9267f0e25 fails this model with E0277. The user
        // authorized this independent compilation fix after that reproduction.
        for (schemaSerde in listOf<Boolean?>(null, false)) {
            generate(schemaSerde)
        }
    }

    @Test
    fun `schema cbor ignores HTTP error bindings`() {
        generate(true)
    }

    private fun generate(schemaSerde: Boolean?) {
        val settings = ServerAdditionalSettings.builder().withHttp1x(true)
        if (schemaSerde != null) {
            settings.schemaSerde(schemaSerde)
        }
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = settings.toObjectNode(),
                cargoCommand = "cargo test --quiet --lib",
            ),
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { context, crate ->
            crate.testModule {
                tokioTest("error_header_members_remain_body_data") {
                    rustTemplate(
                        """
                        let service = crate::Example::builder(crate::ExampleConfig::builder().build())
                            .write(|_: crate::input::WriteInput| async {
                                #{Err}::<crate::output::WriteOutput, crate::error::WriteError>(
                                    crate::error::Failure::builder()
                                        .message(#{Some}("failure".into()))
                                        .detail(#{Some}("header value".into())).build().into())
                            }).build().unwrap();
                        let request = #{Http}::Request::post("/service/Example/operation/Write")
                            .header("smithy-protocol", "rpc-v2-cbor")
                            .header("content-type", "application/cbor")
                            .body(#{Server}::body::to_boxed_sync("")).unwrap();
                        let response = #{Tower}::ServiceExt::oneshot(service, request).await.unwrap();
                        assert_eq!(response.status().as_u16(), 400);
                        assert!(!response.headers().contains_key("x-error"));
                        let body = #{BodyUtil}::BodyExt::collect(response.into_body()).await.unwrap().to_bytes();
                        for value in [b"detail".as_slice(), b"header value".as_slice()] {
                            assert!(body.windows(value.len()).any(|bytes| bytes == value));
                        }
                        """,
                        *RuntimeType.preludeScope,
                        "Http" to RuntimeType.http(context.runtimeConfig),
                        "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                        "Tower" to ServerCargoDependency.tower(context).toType(),
                        "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                    )
                }
            }
        }
    }
}
