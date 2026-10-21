/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class TrailingSlashLabelCompatibilityTest {
    @Test
    fun `legacy labels ignore trailing template slashes`() = checkLabels(schemaSerde = false)

    @Test
    fun `schema labels match legacy trailing template slashes`() = checkLabels(schemaSerde = true)

    private fun checkLabels(schemaSerde: Boolean) {
        for (protocol in listOf("restJson1", "restXml")) {
            val model =
                """
                ${'$'}version: "2"
                namespace test.trailinglabels
                use aws.protocols#$protocol
                @$protocol
                service Labels { version: "1", operations: [Pet, Data, File] }
                @http(method: "GET", uri: "/pets/{name}/")
                operation Pet {
                    input := { @required @httpLabel name: String }
                    output: LabelOutput
                }
                @http(method: "GET", uri: "/data/{key+}/")
                operation Data {
                    input := { @required @httpLabel key: String }
                    output: LabelOutput
                }
                @http(method: "GET", uri: "/files/{key+}/meta/")
                operation File {
                    input := { @required @httpLabel key: String }
                    output: LabelOutput
                }
                structure LabelOutput { @httpPayload value: String }
                """.asSmithyModel()
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ServerAdditionalSettings.builder().schemaSerde(schemaSerde).toObjectNode(),
                    cargoCommand = "cargo test --quiet --lib -- --nocapture",
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                crate.testModule {
                    tokioTest("trailing_template_slashes_match_legacy") {
                        rustTemplate(
                            """
                            let config = crate::service::LabelsConfig::builder().build();
                            let service = crate::service::Labels::builder(config)
                                .pet(|input: crate::input::PetInput| async move {
                                    #{Ok}::<_, crate::error::PetError>(
                                        crate::output::PetOutput::builder().value(#{Some}(input.name)).build())
                                })
                                .data(|input: crate::input::DataInput| async move {
                                    #{Ok}::<_, crate::error::DataError>(
                                        crate::output::DataOutput::builder().value(#{Some}(input.key)).build())
                                })
                                .file(|input: crate::input::FileInput| async move {
                                    #{Ok}::<_, crate::error::FileError>(
                                        crate::output::FileOutput::builder().value(#{Some}(input.key)).build())
                                })
                                .build_unchecked();
                            for (path, status, label) in [
                                ("/pets/rex", 200, #{Some}("rex")),
                                ("/pets/rex%20jr", 200, #{Some}("rex jr")),
                                ("/pets/rex/", 404, #{None}),
                                ("/data/a/b", 200, #{Some}("a/b")),
                                ("/data/a/b/", 200, #{Some}("a/b/")),
                                ("/data//a//b/", 200, #{Some}("/a//b/")),
                                ("/data/", 200, #{Some}("")),
                                ("/data", 404, #{None}),
                                ("/files/a/b/meta", 200, #{Some}("a/b")),
                                ("/files//meta", 200, #{Some}("")),
                                ("/files/a/b/meta/", 404, #{None}),
                            ] {
                                let request = #{Http}::Request::builder().method("GET").uri(path)
                                    .body(#{Server}::body::to_boxed_sync("")).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(service.clone(), request).await.unwrap();
                                let actual_status = response.status().as_u16();
                                let headers = response.headers().clone();
                                let body = #{HttpBodyUtil}::BodyExt::collect(response.into_body())
                                    .await.unwrap().to_bytes();
                                println!("$protocol schemaSerde=$schemaSerde {path}: {actual_status} {headers:?} {body:?}");
                                assert_eq!(actual_status, status, "{path}: {body:?}");
                                if let #{Some}(label) = label {
                                    assert_eq!(headers["content-type"], "text/plain", "{path}");
                                    assert_eq!(headers["content-length"], label.len().to_string(), "{path}");
                                    assert_eq!(body.as_ref(), label.as_bytes(), "{path}");
                                } else if "$protocol" == "restJson1" {
                                    assert_eq!(headers["content-type"], "application/json", "{path}");
                                    assert_eq!(headers["x-amzn-errortype"], "UnknownOperationException", "{path}");
                                    assert_eq!(body.as_ref(), b"{}", "{path}");
                                } else {
                                    assert_eq!(headers["content-type"], "application/xml", "{path}");
                                    assert!(body.is_empty(), "{path}: {body:?}");
                                }
                            }
                            """,
                            *preludeScope,
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "HttpBodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                        )
                    }
                }
            }
        }
    }
}
