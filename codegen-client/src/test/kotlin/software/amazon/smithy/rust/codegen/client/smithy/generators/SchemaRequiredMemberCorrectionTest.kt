/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.customizations.SchemaSerdeAllowlist
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

/**
 * Client error correction on the schema-serde buffered success path.
 *
 * Smithy requires a client to recover from a `@required` member missing from a response rather
 * than failing the call (https://smithy.io/2.0/spec/aggregate-types.html#client-error-correction).
 *
 * The buffered success path builds the output through `builderInstantiator().finalizeBuilder`, the
 * same canonical `correct_*_errors` function the legacy path uses, instead of the defaults that
 * `SchemaGenerator` applies inside `Foo::deserialize`. The two are not equivalent: `SchemaGenerator`
 * has no default for an enum-shaped member, so before this path existed a response omitting a
 * `@required` enum failed to build. `correct_*_errors` parses an unknown variant instead. This test
 * exists because no generated client in the repository has an output with a `@required` member, so
 * nothing else exercises the difference.
 */
class SchemaRequiredMemberCorrectionTest {
    private val model =
        """
        namespace smithy.rust.codegen.test.schemaheaders

        use aws.protocols#restJson1

        @restJson1
        service CorrectionService {
            version: "2023-01-01",
            operations: [GetThing],
        }

        @http(uri: "/thing", method: "GET")
        operation GetThing {
            output: GetThingOutput,
        }

        structure GetThingOutput {
            @required
            name: String,

            @required
            status: Status,

            @required
            tags: TagList,
        }

        enum Status {
            ACTIVE = "ACTIVE"
        }

        list TagList {
            member: String
        }
        """.asSmithyModel(smithyVersion = "2.0")

    @Test
    fun `a required member missing from the response is corrected rather than failing the call`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            check(SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)) {
                "this test must exercise the schema-exclusive buffered success path"
            }
            rustCrate.testModule {
                tokioTest("required_members_absent_from_the_body_are_error_corrected") {
                    rustTemplate(
                        """
                        // An empty JSON object: none of the three required members is present.
                        let http_client = #{infallible_client_fn}(|_req| {
                            #{http_1x}::Response::builder()
                                .status(200)
                                .body(#{SdkBody}::from("{}"))
                                .unwrap()
                        });
                        let config = crate::Config::builder()
                            .http_client(http_client)
                            .endpoint_url("http://localhost:1234")
                            .build();
                        let client = crate::Client::from_conf(config);

                        let output = client
                            .get_thing()
                            .send()
                            .await
                            .expect("error correction must recover from missing required members");

                        // String and list corrections come from `Default::default()`.
                        assert_eq!("", output.name());
                        assert_eq!(0, output.tags().len());
                        // The enum correction is an unknown variant, which is the row that
                        // `SchemaGenerator`'s own defaults could not produce.
                        assert_eq!(
                            "no value was set",
                            output.status().as_str(),
                        );
                        """,
                        *RuntimeType.preludeScope,
                        "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
                        "http_1x" to CargoDependency.Http1x.toType(),
                        "infallible_client_fn" to
                            CargoDependency.smithyHttpClientTestUtil(codegenContext.runtimeConfig)
                                .toType().resolve("test_util::infallible_client_fn"),
                    )
                }
            }
        }
    }
}
