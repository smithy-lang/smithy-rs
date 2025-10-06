/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.client.smithy.traits.IsTruncatedPaginatorTrait
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rustsdk.AwsRuntimeType
import software.amazon.smithy.rustsdk.awsSdkIntegrationTest

class IsTruncatedPaginatorTest {
    private val model =
        """
        namespace test

        use aws.protocols#restXml
        use aws.api#service
        use smithy.rules#endpointRuleSet

        @restXml
        @service(sdkId: "fake")
        @endpointRuleSet({
            "version": "1.0",
            "rules": [{ "type": "endpoint", "conditions": [], "endpoint": { "url": "https://example.com" } }],
            "parameters": {
                "Region": { "required": false, "type": "String", "builtIn": "AWS::Region" },
            }
        })
        service TestService {
            operations: [PaginatedList]
        }

        @readonly
        @optionalAuth
        @http(uri: "/PaginatedList", method: "POST")
        @paginated(inputToken: "nextToken", outputToken: "nextToken",
                   pageSize: "maxResults", items: "items")
        operation PaginatedList {
            input: GetFoosInput,
            output: GetFoosOutput
        }

        structure GetFoosInput {
            maxResults: Integer,
            nextToken: String
        }

        structure GetFoosOutput {
            nextToken: String,
            items: StringList,
            isTruncated: Boolean,
        }

        list StringList {
            member: String
        }
        """.asSmithyModel()

    @Test
    fun `isTruncated paginators work`() {
        // Adding IsTruncated trait to the output shape
        val modifiedModel =
            ModelTransformer.create().mapShapes(model) { shape ->
                shape.letIf(shape.isStructureShape && shape.toShapeId() == ShapeId.from("test#GetFoosOutput")) {
                    (it as StructureShape).toBuilder().addTrait(IsTruncatedPaginatorTrait()).build()
                }
            }

        awsSdkIntegrationTest(modifiedModel) { context, rustCrate ->
            val rc = context.runtimeConfig
            val moduleName = context.moduleUseName()
            rustCrate.integrationTest("is_truncated_paginator") {
                rustTemplate(
                    """
                    ##![cfg(feature = "test-util")]

                    use $moduleName::Config;
                    use $moduleName::Client;
                    use #{Region};
                    use aws_smithy_runtime::client::http::test_util::{ReplayEvent, StaticReplayClient};
                    use aws_smithy_types::body::SdkBody;

                    fn mk_response(part_marker: u8) -> http::Response<SdkBody> {
                        let (part_num_marker, next_num_marker, is_truncated) = if part_marker < 3 {
                            (part_marker, part_marker + 1, true)
                        } else {
                            (part_marker, 0, false)
                        };
                        let body = format!(
                            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n
                        <GetFoosOutput
                            xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">
                            <token>{part_num_marker}</token>
                            <nextToken>{next_num_marker}</nextToken>
                            <isTruncated>{is_truncated}</isTruncated>
                        </GetFoosOutput>"
                        );
                        http::Response::builder().body(SdkBody::from(body)).unwrap()
                    }

                    fn mk_request() -> http::Request<SdkBody> {
                        http::Request::builder()
                            .uri("https://some-test-bucket.s3.us-east-1.amazonaws.com/test.txt?part-number-marker=PartNumberMarker&uploadId=UploadId")
                            .body(SdkBody::empty())
                            .unwrap()
                    }

                    ##[#{tokio}::test]
                    async fn is_truncated_pagination_does_not_loop() {
                        let http_client = StaticReplayClient::new(vec![
                            ReplayEvent::new(mk_request(), mk_response(0)),
                            ReplayEvent::new(mk_request(), mk_response(1)),
                            ReplayEvent::new(mk_request(), mk_response(2)),
                            ReplayEvent::new(mk_request(), mk_response(3)),
                            //The events below should never be called because the pagination should
                            //terminate with the event above
                            ReplayEvent::new(mk_request(), mk_response(0)),
                            ReplayEvent::new(mk_request(), mk_response(1)),
                        ]);

                        let config = Config::builder()
                            .region(Region::new("fake"))
                            .http_client(http_client.clone())
                            .with_test_defaults()
                            .build();
                        let client = Client::from_conf(config);

                        let list_parts_res = client
                            .paginated_list()
                            .max_results(1)
                            .into_paginator()
                            .send()
                            .collect::<Vec<_>>()
                            .await;

                        // Confirm that the pagination stopped calling the http client after the
                        // first page with is_truncated = false
                        assert_eq!(list_parts_res.len(), 4)
                    }
                    """,
                    *preludeScope,
                    "tokio" to CargoDependency.Tokio.toType(),
                    "Region" to AwsRuntimeType.awsTypes(rc).resolve("region::Region"),
                )
            }
        }
    }
}
