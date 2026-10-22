/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import io.kotest.matchers.shouldBe
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files
import kotlin.io.path.readText

internal class RestBodyBindingFilteringTest {
    @ParameterizedTest
    @ValueSource(strings = ["restJson1", "restXml"])
    fun `body copies of transport members preserve legacy bindings`(protocol: String) {
        val model =
            """
            ${'$'}version: "2"
            namespace test.bodybindings
            use aws.protocols#$protocol
            use smithy.framework#ValidationException

            @$protocol
            service Orders { version: "1", operations: [CreateOrder] }

            @http(method: "POST", uri: "/orders/{storeId}", code: 201)
            operation CreateOrder {
                input: CreateOrderInput
                output := { @httpPayload value: String }
                errors: [ValidationException]
            }

            @xmlName("OrderDocument")
            structure CreateOrderInput {
                @required @httpLabel storeId: String
                @httpHeader("x-tenant-id") tenantId: String
                @httpQuery("dryRun") dryRun: Boolean
                note: String
                quantity: Integer
                detail: Detail
            }

            structure Detail { tenantId: String }
            """.asSmithyModel()
        val mediaType = if (protocol == "restJson1") "application/json" else "application/xml"
        val bodies =
            if (protocol == "restJson1") {
                listOf(
                    """{"note":"hi","quantity":2,"detail":{"tenantId":"nested"}}""",
                    """{"storeId":"evil","tenantId":"tenant-B","dryRun":true,"note":"hi","quantity":2,"detail":{"tenantId":"nested"}}""",
                    """{"storeId":[],"tenantId":{},"dryRun":"yes-please","note":"hi","quantity":2,"detail":{"tenantId":"nested"}}""",
                    """{"note":"hi","quantity":2,"detail":{"tenantId":"nested"},"tenantId":[{"note":"decoy"}],"dryRun":{}}""",
                )
            } else {
                listOf(
                    "<OrderDocument><note>hi</note><quantity>2</quantity><detail><tenantId>nested</tenantId></detail></OrderDocument>",
                    "<OrderDocument><storeId>evil</storeId><tenantId>tenant-B</tenantId><dryRun>true</dryRun><note>hi</note><quantity>2</quantity><detail><tenantId>nested</tenantId></detail></OrderDocument>",
                    "<OrderDocument><storeId><ignored/></storeId><tenantId><ignored/></tenantId><dryRun>yes-please</dryRun><note>hi</note><quantity>2</quantity><detail><tenantId>nested</tenantId></detail></OrderDocument>",
                    "<OrderDocument><note>hi</note><quantity>2</quantity><detail><tenantId>nested</tenantId></detail><tenantId><note>decoy</note></tenantId><dryRun><ignored/></dryRun></OrderDocument>",
                )
            }
        val cases = bodies.joinToString(",\n") { it.dq() }
        val observations = Files.createTempDirectory("smithy-body-bindings-$protocol-")
        for (schemaSerde in listOf(false, true)) {
            val snapshot = observations.resolve("$schemaSerde.txt")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":$schemaSerde}}""").expectObjectNode(),
                    cargoCommand = "cargo test --quiet --lib --tests",
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                crate.testModule {
                    tokioTest("body_copies_cannot_assign_transport_members") {
                        rustTemplate(
                            """
                            use #{Tower}::ServiceExt;
                            use #{BodyUtil}::BodyExt;
                            let service = crate::Orders::builder(crate::OrdersConfig::builder().build())
                                .create_order(|input: crate::input::CreateOrderInput| async move {
                                    #{Ok}::<_, crate::error::CreateOrderError>(crate::output::CreateOrderOutput {
                                        value: #{Some}(format!("{}|{}|{}|{}|{}|{}",
                                            input.store_id,
                                            input.tenant_id.as_deref().unwrap_or("unset"),
                                            input.dry_run.unwrap_or(false),
                                            input.note.as_deref().unwrap_or("unset"),
                                            input.quantity.unwrap_or_default(),
                                            input.detail.and_then(|detail| detail.tenant_id).as_deref().unwrap_or("unset"),
                                        )),
                                    })
                                })
                                .build().unwrap();
                            let mut observations = #{String}::new();
                            for body in [$cases] {
                                for with_header in [true, false] {
                                    let mut request = #{Http}::Request::builder()
                                        .method("POST").uri("/orders/store-7?dryRun=false")
                                        .header("content-type", ${mediaType.dq()});
                                    if with_header {
                                        request = request.header("x-tenant-id", "tenant-A");
                                    }
                                    let request = request.body(#{Server}::body::to_boxed_sync(body.as_bytes().to_vec())).unwrap();
                                    let response = service.clone().oneshot(request).await.unwrap();
                                    let status = response.status().as_u16();
                                    let mut headers: #{Vec}<_> = response.headers().iter()
                                        .map(|(name, value)| (name.as_str().to_owned(), value.as_bytes().to_vec())).collect();
                                    headers.sort();
                                    let bytes = response.into_body().collect().await.unwrap().to_bytes();
                                    assert_eq!(status, 201, "{body}");
                                    let tenant = if with_header { "tenant-A" } else { "unset" };
                                    assert_eq!(bytes.as_ref(), format!("store-7|{tenant}|false|hi|2|nested").as_bytes(), "{body}");
                                    observations.push_str(&format!("{status} {headers:?} {bytes:?}\n"));
                                }
                            }
                            ::std::fs::write(${snapshot.toString().dq()}, observations).unwrap();
                            """,
                            *RuntimeType.preludeScope,
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                        )
                    }
                }
            }
        }
        observations.resolve("true.txt").readText() shouldBe observations.resolve("false.txt").readText()
    }
}
