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
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import java.nio.file.Files
import kotlin.io.path.extension
import kotlin.io.path.readText

class DocumentV2DecoratorTest {
    /**
     * The `Foo` model. In [SCHEMA_SERDE_NAMESPACE], which `SchemaSerdeAllowlist` lists, the client uses schema serde
     * only. Other namespaces use the `protocol_serde` serializers.
     */
    private fun model(namespace: String) =
        """
        ${'$'}version: "2"
        namespace $namespace

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
            clientIntegrationTest(model(SCHEMA_SERDE_NAMESPACE)) { codegenContext, rustCrate ->
                requireSchemaSerde(codegenContext)
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

    @Test
    fun `schema serde writes and reads the DocumentV2 companion`() {
        val crateDir =
            clientIntegrationTest(model(SCHEMA_SERDE_NAMESPACE)) { codegenContext, rustCrate ->
                requireSchemaSerde(codegenContext)
                val runtimeConfig = codegenContext.runtimeConfig
                val smithySchema = RuntimeType.smithySchema(runtimeConfig)
                rustCrate.testModule {
                    rustTemplate(
                        """
                        use crate::operation::foo::{FooInput, FooOutput};
                        use crate::types::error::FooError;
                        use crate::types::{FooChoice, FooData};
                        use #{Codec};
                        use #{SerializableStruct};
                        use #{ShapeDeserializer};
                        use #{ShapeSerializer};
                        use #{BigDecimal};
                        use #{BigInteger};
                        use #{Document};
                        use #{DocumentObject};
                        use #{DocumentV2};
                        use std::str::FromStr;

                        const BIG: &str = "123456789012345678901234567890";

                        // JSON writes this value as a number, and `Document` holds it as the nearest `f64`.
                        fn big() -> DocumentV2 {
                            DocumentV2::BigInteger(BigInteger::from_str(BIG).unwrap())
                        }

                        fn v1(value: &str) -> Document {
                            Document::from(value)
                        }

                        fn json(value: &dyn SerializableStruct) -> String {
                            let mut ser = #{JsonCodec}::default().create_serializer();
                            ser.write_struct(value.schema(), value).unwrap();
                            String::from_utf8(ser.finish()).unwrap()
                        }

                        fn deser(json: &str) -> impl ShapeDeserializer + '_ {
                            #{JsonCodec}::default().create_deserializer(json.as_bytes())
                        }

                        // What reading `json` as a `Document` gives, which is what the public field held before
                        // `DocumentV2`.
                        fn read_v1(json: &str) -> Document {
                            deser(json).read_document(&#{DocumentSchema}).unwrap()
                        }

                        fn big_v1() -> Document {
                            read_v1(BIG)
                        }

                        ##[test]
                        fn the_document_field_is_written_without_a_companion() {
                            let input = FooInput::builder().payload(v1("v1")).build().unwrap();
                            assert_eq!(json(&input), r##"{"payload":"v1"}"##);
                        }

                        ##[test]
                        fn the_companion_is_written_in_place_of_the_document_field() {
                            for builder in [
                                FooInput::builder().payload_v2(big()),
                                FooInput::builder().payload(v1("v1")).payload_v2(big()),
                                FooInput::builder().payload_v2(big()).payload(v1("v1")),
                            ] {
                                let input = builder.build().unwrap();
                                assert_eq!(input.payload, Some(big_v1()));
                                assert_eq!(json(&input), format!(r##"{{"payload":{BIG}}}"##));
                            }
                        }

                        ##[test]
                        fn clearing_the_companion_writes_the_document_field() {
                            let input = FooInput::builder()
                                .payload(v1("v1"))
                                .payload_v2(big())
                                .set_payload_v2(None)
                                .build()
                                .unwrap();
                            assert_eq!(json(&input), r##"{"payload":"v1"}"##);
                        }

                        ##[test]
                        fn changing_the_public_field_changes_the_output_only_without_a_companion() {
                            let mut input = FooInput::builder().payload(v1("before")).build().unwrap();
                            input.payload = Some(v1("after"));
                            assert_eq!(json(&input), r##"{"payload":"after"}"##);

                            let mut input = FooInput::builder().payload_v2(big()).build().unwrap();
                            input.payload = Some(v1("after"));
                            assert_eq!(json(&input), format!(r##"{{"payload":{BIG}}}"##));
                        }

                        ##[test]
                        fn object_members_keep_their_order() {
                            let object = DocumentObject::from([
                                ("z".to_string(), DocumentV2::from(1u64)),
                                ("a".to_string(), DocumentV2::from(2u64)),
                            ]);
                            let input = FooInput::builder().payload_v2(object.into()).build().unwrap();
                            assert_eq!(json(&input), r##"{"payload":{"z":1,"a":2}}"##);
                        }

                        ##[test]
                        fn collections_write_their_companions() {
                            let input = FooInput::builder()
                                .payload_list_v2(big())
                                .payload_map_v2("key", big())
                                .set_sparse_payload_list_v2(Some(vec![Some(big()), None]))
                                .nested_payload_list_v2(vec![big()])
                                .build()
                                .unwrap();
                            assert_eq!(
                                json(&input),
                                format!(
                                    r##"{{"payloadList":[{BIG}],"payloadMap":{{"key":{BIG}}},"sparsePayloadList":[{BIG},null],"nestedPayloadList":[[{BIG}]]}}"##
                                ),
                            );
                        }

                        ##[test]
                        fn nested_structures_and_errors_write_their_companions() {
                            let data = FooData::builder().body_v2(big()).type_v2(big()).build().unwrap();
                            let input = FooInput::builder().data(data).build().unwrap();
                            assert_eq!(
                                json(&input),
                                format!(r##"{{"data":{{"body":{BIG},"type":{BIG}}}}}"##),
                            );

                            let error = FooError::builder().details_v2(big()).build();
                            assert_eq!(json(&error), format!(r##"{{"details":{BIG}}}"##));
                        }

                        ##[test]
                        fn union_variants_write_the_document() {
                            let input = FooInput::builder().choice(FooChoice::Doc(v1("v1"))).build().unwrap();
                            assert_eq!(json(&input), r##"{"choice":{"doc":"v1"}}"##);
                        }

                        ##[test]
                        fn a_member_is_read_into_its_companion_and_the_field_matches_read_document() {
                            let document = r##"{"pi":3.14159265358979323846,"big":123456789012345678901234567890,"small":42}"##;
                            let output = FooOutput::deserialize(&mut deser(&format!(r##"{{"payload":{document}}}"##))).unwrap();

                            let payload = output.payload_v2().unwrap().as_object().unwrap();
                            assert_eq!(payload.keys().map(String::as_str).collect::<Vec<_>>(), ["pi", "big", "small"]);
                            assert_eq!(
                                payload["pi"],
                                DocumentV2::BigDecimal(BigDecimal::from_str("3.14159265358979323846").unwrap())
                            );
                            assert_eq!(payload["big"], big());
                            assert_eq!(payload["small"], DocumentV2::from(42u64));

                            // The public field holds what reading a `Document` gives, without a getter call.
                            assert_eq!(output.payload, Some(read_v1(document)));
                            assert_eq!(output.payload(), Some(&read_v1(document)));

                            // An explicit null leaves both empty, as it left the field before.
                            let output = FooOutput::deserialize(&mut deser(r##"{"payload":null}"##)).unwrap();
                            assert_eq!(output.payload, None);
                            assert_eq!(output.payload_v2(), None);
                        }

                        ##[test]
                        fn collections_nested_structures_and_errors_read_into_their_companions() {
                            let input = FooInput::deserialize(&mut deser(&format!(
                                r##"{{"payloadList":[{BIG}],"payloadMap":{{"key":{BIG}}},"sparsePayloadList":[{BIG},null],"nestedPayloadList":[[{BIG}]],"data":{{"body":{BIG}}}}}"##
                            )))
                            .unwrap();
                            assert_eq!(input.payload_list_v2(), Some(&[big()][..]));
                            assert_eq!(input.payload_list, Some(vec![big_v1()]));
                            assert_eq!(input.payload_map_v2().unwrap()["key"], big());
                            assert_eq!(input.payload_map.as_ref().unwrap()["key"], big_v1());
                            assert_eq!(input.sparse_payload_list_v2(), Some(&[Some(big()), None][..]));
                            assert_eq!(input.sparse_payload_list, Some(vec![Some(big_v1()), None]));
                            assert_eq!(input.nested_payload_list_v2(), Some(&[vec![big()]][..]));
                            assert_eq!(input.nested_payload_list, Some(vec![vec![big_v1()]]));
                            let data = input.data.unwrap();
                            assert_eq!(data.body_v2(), Some(&big()));
                            assert_eq!(data.body, big_v1());

                            let error = FooError::deserialize(&mut deser(&format!(r##"{{"details":{BIG}}}"##))).unwrap();
                            assert_eq!(error.details_v2(), Some(&big()));
                            assert_eq!(error.details, Some(big_v1()));
                        }

                        ##[test]
                        fn union_variants_read_the_document() {
                            let input = FooInput::deserialize(&mut deser(&format!(r##"{{"choice":{{"doc":{BIG}}}}}"##))).unwrap();
                            assert_eq!(input.choice, Some(FooChoice::Doc(big_v1())));
                        }

                        ##[test]
                        fn a_value_read_is_written_back_unchanged() {
                            let text = format!(r##"{{"payload":{{"z":{BIG},"a":0.1234567890123456789}}}}"##);
                            let input = FooInput::deserialize(&mut deser(&text)).unwrap();
                            assert_eq!(json(&input), text);
                        }
                        """,
                        "Codec" to smithySchema.resolve("codec::Codec"),
                        "SerializableStruct" to smithySchema.resolve("serde::SerializableStruct"),
                        "ShapeDeserializer" to smithySchema.resolve("serde::ShapeDeserializer"),
                        "ShapeSerializer" to smithySchema.resolve("serde::ShapeSerializer"),
                        "DocumentSchema" to smithySchema.resolve("prelude::DOCUMENT"),
                        "JsonCodec" to RuntimeType.smithyJson(runtimeConfig).resolve("codec::JsonCodec"),
                        "BigDecimal" to RuntimeType.bigDecimal(runtimeConfig),
                        "BigInteger" to RuntimeType.bigInteger(runtimeConfig),
                        "Document" to RuntimeType.document(runtimeConfig),
                        "DocumentObject" to RuntimeType.smithyTypes(runtimeConfig).resolve("document::DocumentObject"),
                        "DocumentV2" to RuntimeType.documentV2(runtimeConfig),
                    )

                    tokioTest("the_client_sends_the_companion") {
                        rustTemplate(
                            """
                            let (http_client, request) = #{capture_request}(None);
                            let config = crate::Config::builder()
                                .endpoint_url("http://localhost:1234")
                                .http_client(http_client)
                                .behavior_version_latest()
                                .build();
                            let client = crate::Client::from_conf(config);
                            let _ = client.foo().payload(v1("v1")).payload_v2(big()).send().await;
                            let request = request.expect_request();
                            let body = std::str::from_utf8(request.body().bytes().unwrap()).unwrap();
                            assert_eq!(body, format!(r##"{{"payload":{BIG}}}"##));
                            """,
                            "capture_request" to RuntimeType.captureRequest(runtimeConfig),
                        )
                    }

                    tokioTest("the_client_reads_the_companion") {
                        rustTemplate(
                            """
                            let response = #{http_1x}::Response::builder()
                                .status(200)
                                .body(#{SdkBody}::from(format!(r##"{{"payload":{BIG}}}"##)))
                                .unwrap();
                            let (http_client, _request) = #{capture_request}(Some(response));
                            let config = crate::Config::builder()
                                .endpoint_url("http://localhost:1234")
                                .http_client(http_client)
                                .behavior_version_latest()
                                .build();
                            let client = crate::Client::from_conf(config);
                            let output = client.foo().send().await.unwrap();
                            assert_eq!(output.payload_v2(), Some(&big()));
                            assert_eq!(output.payload, Some(big_v1()));
                            """,
                            "capture_request" to RuntimeType.captureRequest(runtimeConfig),
                            "http_1x" to CargoDependency.Http1x.toType(),
                            "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
                        )
                    }
                }
            }

        val sources =
            Files.walk(crateDir.resolve("src")).use { paths ->
                paths.filter { it.extension == "rs" }.map { it.readText() }.toList()
            }.joinToString("\n")
        // Union variants stay `Document` only.
        sources shouldContain "ser.write_document(&FOOCHOICE_MEMBER_DOC, val)?;"
    }

    @Test
    fun `clients without schema serde get no companions`() {
        val crateDir =
            clientIntegrationTest(model("com.example")) { codegenContext, _ ->
                check(!SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)) {
                    "this test must generate a client without schema serde; check that com.example isn't in " +
                        "SchemaSerdeAllowlist"
                }
            }

        val sources =
            Files.walk(crateDir.resolve("src")).use { paths ->
                paths.filter { it.extension == "rs" }.map { it.readText() }.toList()
            }.joinToString("\n")
        sources shouldNotContain "DocumentV2"
        sources shouldNotContain "payload_v2"
        // Structures with documents keep their `Debug` derive.
        sources shouldContain Regex("""derive\([^)]*::std::fmt::Debug[^)]*\)]\s*pub struct FooData """)
    }

    private fun requireSchemaSerde(codegenContext: ClientCodegenContext) {
        check(SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)) {
            "this test must generate schema serde; check that $SCHEMA_SERDE_NAMESPACE is still in SchemaSerdeAllowlist"
        }
    }

    companion object {
        private const val SCHEMA_SERDE_NAMESPACE = "smithy.rust.codegen.test.schemaheaders"
    }
}
