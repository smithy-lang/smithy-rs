/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import io.kotest.matchers.collections.shouldNotBeEmpty
import io.kotest.matchers.shouldBe
import io.kotest.matchers.string.shouldContain
import io.kotest.matchers.string.shouldNotContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import java.nio.file.Files
import kotlin.io.path.extension
import kotlin.io.path.readText

class DocumentV2DecoratorTest {
    private val model =
        """
        ${'$'}version: "2"
        namespace com.example

        use aws.protocols#awsJson1_0

        @awsJson1_0
        service FooService {
            version: "1"
            operations: [Foo, FooStream]
        }

        operation Foo {
            input := {
                payload: Document
                payloadList: DocumentList
                payloadMap: DocumentMap
                sparsePayloadList: SparseDocumentList
                nestedPayloadList: DocumentListList
                data: FooData
                choice: FooChoice
                name: String
            }
            output := {
                payload: Document
                data: FooData
            }
            errors: [FooError]
        }

        operation FooStream {
            output := {
                events: Events
                payload: Document
            }
        }

        @streaming
        union Events {
            tick: Tick
        }

        structure Tick {
            value: String
        }

        structure FooData {
            @required
            body: Document

            @deprecated
            legacy: Document

            type: Document
        }

        union FooChoice {
            doc: Document
            text: String
        }

        @error("client")
        structure FooError {
            message: String
            details: Document
        }

        list DocumentList {
            member: Document
        }

        map DocumentMap {
            key: String
            value: Document
        }

        @sparse
        list SparseDocumentList {
            member: Document
        }

        list DocumentListList {
            member: DocumentList
        }
        """.asSmithyModel()

    @Test
    fun `document members get DocumentV2 companions`() {
        val crateDir =
            clientIntegrationTest(model) { codegenContext, rustCrate ->
                val moduleName = codegenContext.moduleUseName()
                rustCrate.integrationTest("document_v2") {
                    rustTemplate(
                        """
                        use $moduleName::operation::foo::{FooInput, FooOutput};
                        use $moduleName::types::error::FooError;
                        use $moduleName::types::{FooChoice, FooData};
                        use #{Document};
                        use #{DocumentV2};

                        // Converts to the V1 string "AQID".
                        fn blob() -> DocumentV2 {
                            DocumentV2::Blob(vec![1, 2, 3])
                        }

                        fn v1(value: &str) -> Document {
                            Document::from(value)
                        }

                        ##[test]
                        fn a_document_value_has_no_companion() {
                            let input = FooInput::builder().payload(v1("v1")).build().unwrap();
                            assert_eq!(input.payload, Some(v1("v1")));
                            assert_eq!(input.payload_v2(), None);
                        }

                        ##[test]
                        fn a_companion_sets_the_public_field() {
                            let input = FooInput::builder().payload_v2(blob()).build().unwrap();
                            assert_eq!(input.payload_v2(), Some(&blob()));
                            assert_eq!(input.payload, Some(v1("AQID")));
                            assert_eq!(input.payload(), Some(&v1("AQID")));
                        }

                        ##[test]
                        fn the_companion_wins_in_either_order() {
                            for builder in [
                                FooInput::builder().payload(v1("v1")).payload_v2(blob()),
                                FooInput::builder().payload_v2(blob()).payload(v1("v1")),
                            ] {
                                assert_eq!(builder.get_payload(), &Some(v1("v1")));
                                assert_eq!(builder.get_payload_v2(), &Some(blob()));
                                let input = builder.build().unwrap();
                                assert_eq!(input.payload, Some(v1("AQID")));
                                assert_eq!(input.payload_v2(), Some(&blob()));
                            }
                        }

                        ##[test]
                        fn clearing_the_companion_restores_the_document_value() {
                            let builder = FooInput::builder()
                                .payload(v1("v1"))
                                .payload_v2(blob())
                                .set_payload_v2(None);
                            assert_eq!(builder.get_payload_v2(), &None);
                            let input = builder.build().unwrap();
                            assert_eq!(input.payload, Some(v1("v1")));
                            assert_eq!(input.payload_v2(), None);
                        }

                        ##[test]
                        fn collections_convert_each_document() {
                            let input = FooInput::builder()
                                .payload_list_v2(blob())
                                .payload_list_v2(DocumentV2::from(true))
                                .payload_map_v2("key", blob())
                                .set_sparse_payload_list_v2(Some(vec![Some(blob()), None]))
                                .nested_payload_list_v2(vec![blob()])
                                .build()
                                .unwrap();
                            assert_eq!(input.payload_list, Some(vec![v1("AQID"), Document::from(true)]));
                            assert_eq!(input.payload_list_v2(), Some(&[blob(), DocumentV2::from(true)][..]));
                            assert_eq!(input.payload_map.as_ref().unwrap().get("key"), Some(&v1("AQID")));
                            assert_eq!(input.payload_map_v2().unwrap().get("key"), Some(&blob()));
                            assert_eq!(input.sparse_payload_list, Some(vec![Some(v1("AQID")), None]));
                            assert_eq!(input.sparse_payload_list_v2(), Some(&[Some(blob()), None][..]));
                            assert_eq!(input.nested_payload_list, Some(vec![vec![v1("AQID")]]));
                            assert_eq!(input.nested_payload_list_v2(), Some(&[vec![blob()]][..]));
                        }

                        ##[test]
                        fn either_form_satisfies_a_required_member() {
                            let data = FooData::builder().body(v1("v1")).build().unwrap();
                            assert_eq!(data.body, v1("v1"));
                            assert_eq!(data.body_v2(), None);

                            let data = FooData::builder().body_v2(blob()).build().unwrap();
                            assert_eq!(data.body, v1("AQID"));
                            assert_eq!(data.body(), &v1("AQID"));
                            assert_eq!(data.body_v2(), Some(&blob()));

                            let error = FooData::builder().build().unwrap_err();
                            assert!(error.to_string().contains("body"), "{error}");
                        }

                        ##[test]
                        fn equality_and_debug_ignore_the_companion() {
                            let from_v1 = FooData::builder().body(v1("AQID")).build().unwrap();
                            let from_v2 = FooData::builder().body_v2(blob()).build().unwrap();
                            assert_eq!(from_v1, from_v2);
                            assert_eq!(format!("{from_v1:?}"), format!("{from_v2:?}"));
                            assert_eq!(
                                format!("{from_v2:?}"),
                                r##"FooData { body: String("AQID"), legacy: None, r##type: None }"##
                            );

                            // Builders compare and print their own fields, and leave out the companion in `Debug`.
                            let builder = FooData::builder().body(v1("AQID"));
                            assert_ne!(builder.clone().body_v2(blob()), builder);
                            assert_eq!(format!("{:?}", builder.clone().body_v2(blob())), format!("{builder:?}"));
                        }

                        ##[test]
                        fn raw_identifier_members_get_plain_companion_names() {
                            let data = FooData::builder().body(v1("v1")).type_v2(blob()).build().unwrap();
                            assert_eq!(data.r##type, Some(v1("AQID")));
                            assert_eq!(data.type_v2(), Some(&blob()));
                        }

                        ##[test]
                        fn clone_keeps_the_companion() {
                            let data = FooData::builder().body_v2(blob()).build().unwrap();
                            assert_eq!(data.clone().body_v2(), Some(&blob()));
                        }

                        ##[test]
                        fn public_fields_stay_readable_and_writable() {
                            let mut output = FooOutput::builder().payload_v2(blob()).build();
                            let FooOutput { payload, .. } = &output;
                            assert_eq!(payload, &Some(v1("AQID")));

                            // Changing the public field doesn't change the companion.
                            output.payload = Some(v1("changed"));
                            assert_eq!(output.payload(), Some(&v1("changed")));
                            assert_eq!(output.payload_v2(), Some(&blob()));
                        }

                        ##[test]
                        fn errors_have_companions() {
                            let error = FooError::builder().message("oops").details_v2(blob()).build();
                            assert_eq!(error.details, Some(v1("AQID")));
                            assert_eq!(error.details_v2(), Some(&blob()));
                        }

                        ##[test]
                        fn union_variants_stay_document_only() {
                            let choice = FooChoice::Doc(v1("v1"));
                            assert_eq!(choice.as_doc(), Ok(&v1("v1")));
                        }

                        ##[test]
                        fn fluent_builders_delegate_to_the_input_builder() {
                            let (http_client, _request) = #{capture_request}(None);
                            let config = $moduleName::Config::builder()
                                .endpoint_url("http://localhost:1234")
                                .http_client(http_client)
                                .build();
                            let client = $moduleName::Client::from_conf(config);
                            let fluent = client
                                .foo()
                                .payload(v1("v1"))
                                .payload_v2(blob())
                                .payload_list_v2(blob())
                                .payload_map_v2("key", blob());
                            assert_eq!(fluent.get_payload_v2(), &Some(blob()));
                            assert_eq!(fluent.as_input().get_payload_list_v2(), &Some(vec![blob()]));
                            assert_eq!(fluent.get_payload_map_v2().as_ref().unwrap().get("key"), Some(&blob()));

                            let fluent = fluent.set_payload_v2(None);
                            assert_eq!(fluent.get_payload_v2(), &None);
                            assert_eq!(fluent.get_payload(), &Some(v1("v1")));
                        }
                        """,
                        "Document" to RuntimeType.document(codegenContext.runtimeConfig),
                        "DocumentV2" to RuntimeType.documentV2(codegenContext.runtimeConfig),
                        "capture_request" to RuntimeType.captureRequest(codegenContext.runtimeConfig),
                    )
                }
            }

        val sources =
            Files.walk(crateDir.resolve("src")).use { paths ->
                paths.filter { it.extension == "rs" }.map { it.readText() }.toList()
            }.joinToString("\n")

        // The output of an event stream operation with an initial response gets `into_builder`, which must carry the
        // companion. Compiling it checks the types; this checks that the setter is there.
        sources shouldContain ".set_payload_v2(self.payload_v2.0)"

        // A deprecated member's `DocumentV2` methods are deprecated too.
        val legacyMethods = Regex("""(#\[deprecated]\s*)?pub fn (set_|get_)?legacy_v2\(""").findAll(sources).toList()
        legacyMethods.shouldNotBeEmpty()
        legacyMethods.forEach { it.groupValues[1].isNotEmpty() shouldBe true }

        // Union variants have no companions.
        sources shouldNotContain "doc_v2"
    }
}
