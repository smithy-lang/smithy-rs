/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class XmlUnionListCompatibilityTest {
    @Test
    fun `legacy union list items use list member names`() = checkTags(schemaSerde = false)

    @Test
    fun `schema union list item tags match legacy`() = checkTags(schemaSerde = true)

    private fun checkTags(schemaSerde: Boolean) {
        val model =
            """
            ${'$'}version: "2"
            namespace test.unionlist
            use aws.protocols#restXml
            @restXml
            service Lists { version: "1", operations: [Get] }
            @http(method: "GET", uri: "/list")
            operation Get {
                input := {}
                output: Output
            }
            @xmlName("Result")
            structure Output {
                choices: Choices
                @xmlFlattened flattened: Choices
                renamed: RenamedChoices
            }
            list Choices { member: Choice }
            list RenamedChoices { @xmlName("item") member: Choice }
            @xmlName("ChoiceTargetRoot")
            union Choice { text: String }
            """.asSmithyModel()
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":$schemaSerde}}""").expectObjectNode(),
                cargoCommand = "cargo test --quiet --lib -- --nocapture",
            ),
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { context, crate ->
            crate.testModule {
                tokioTest("union_target_names_do_not_override_list_item_tags") {
                    rustTemplate(
                        """
                        let config = crate::service::ListsConfig::builder().build();
                        let service = crate::service::Lists::builder(config)
                            .get(|_: crate::input::GetInput| async {
                                let item = crate::model::Choice::Text("hello".to_owned());
                                crate::output::GetOutput::builder()
                                    .choices(#{Some}(vec![item.clone()]))
                                    .renamed(#{Some}(vec![item.clone()]))
                                    .flattened(#{Some}(vec![item]))
                                    .build()
                            })
                            .build_unchecked();
                        let request = #{Http}::Request::get("/list")
                            .body(#{Server}::body::Body::empty()).unwrap();
                        let response = #{Tower}::ServiceExt::oneshot(service, request).await.unwrap();
                        let status = response.status().as_u16();
                        let headers = response.headers().clone();
                        let body = #{BodyUtil}::BodyExt::collect(response.into_body()).await.unwrap().to_bytes();
                        println!("schemaSerde=$schemaSerde {status} {headers:?} {body:?}");
                        assert_eq!(status, 200);
                        assert_eq!(headers["content-type"], "application/xml");
                        assert_eq!(body.as_ref(), concat!(
                            "<Result><choices><member><text>hello</text></member></choices>",
                            "<flattened><text>hello</text></flattened>",
                            "<renamed><item><text>hello</text></item></renamed></Result>",
                        ).as_bytes());
                        """,
                        *preludeScope,
                        "Http" to RuntimeType.http(context.runtimeConfig),
                        "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                        "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                        "Tower" to ServerCargoDependency.tower(context).toType(),
                    )
                }
            }
        }
    }
}
