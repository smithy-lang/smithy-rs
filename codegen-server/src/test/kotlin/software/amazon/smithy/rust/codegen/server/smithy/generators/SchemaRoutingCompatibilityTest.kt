/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
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

/** Identical wire requests must select the same operation in both serde modes. */
class SchemaRoutingCompatibilityTest {
    @Test
    fun `REST query literals retain modeled plus and percent characters`() {
        for (protocol in listOf("restJson1", "restXml")) {
            checkRouting(
                protocol,
                "/literal?a+b=c+d&percent=%2B&flag",
                """
                for (query, status) in [
                    ("a%2Bb=c%2Bd&percent=%252B&flag", 200),
                    ("a+b=c+d&percent=%2B&flag", 404),
                    ("a%2Bb=c%2Bd&percent=%252B", 404),
                ] {
                    let request = #{Http}::Request::get(format!("/literal?{query}"))
                        .body(#{Server}::body::Body::empty()).unwrap();
                    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), status, "{query}");
                }
                """,
            )
        }
    }

    @Test
    fun `AWS JSON modeled names always route and capitalized aliases are opt in`() {
        for (protocol in listOf("awsJson1_0", "awsJson1_1")) {
            for (capitalizeRoutes in listOf(null, false, true)) {
                checkRouting(
                    protocol,
                    "/literal",
                    """
                    for (target, status) in [("Example.getFoo", 200), ("Example.GetFoo", ${if (capitalizeRoutes == true) 200 else 404})] {
                        let request = #{Http}::Request::post("/")
                            .header("x-amz-target", target)
                            .header("content-type", "${if (protocol == "awsJson1_0") "application/x-amz-json-1.0" else "application/x-amz-json-1.1"}")
                            .body(#{Server}::body::Body::empty()).unwrap();
                        assert_eq!(app.clone().oneshot(request).await.unwrap().status(), status, "{target}");
                    }
                    """,
                    capitalizeRoutes,
                )
            }
        }
    }

    private fun checkRouting(
        protocol: String,
        uri: String,
        assertions: String,
        capitalizeRoutes: Boolean? = null,
    ) {
        val model =
            """
            namespace test
            use aws.protocols#$protocol
            @$protocol service Example { operations: [getFoo] }
            @http(method: "GET", uri: "$uri")
            operation getFoo { input := {} output := {} }
            """.asSmithyModel(smithyVersion = "2")
        for (schemaSerde in listOf(false, true)) {
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings =
                        ServerAdditionalSettings.builder().schemaSerde(schemaSerde).toObjectNode().let { settings ->
                            if (capitalizeRoutes == null) {
                                settings
                            } else {
                                settings.withMember(
                                    "customizationConfig",
                                    ObjectNode.builder().withMember(
                                        "protocols",
                                        ObjectNode.builder().withMember(
                                            "aws.protocols#$protocol",
                                            ObjectNode.builder().withMember("capitalizeRoutes", capitalizeRoutes).build(),
                                        ).build(),
                                    ).build(),
                                )
                            }
                        },
                    cargoCommand = "cargo test --quiet --lib",
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                crate.testModule {
                    tokioTest("routing_compatibility") {
                        rustTemplate(
                            """
                            use #{Tower}::ServiceExt;
                            let app = crate::Example::builder(crate::ExampleConfig::builder().build())
                                .get_foo(|_: crate::input::GetFooInput| async { crate::output::GetFooOutput {} })
                                .build().unwrap();
                            $assertions
                            """,
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            *RuntimeType.preludeScope,
                        )
                    }
                }
            }
        }
    }
}
