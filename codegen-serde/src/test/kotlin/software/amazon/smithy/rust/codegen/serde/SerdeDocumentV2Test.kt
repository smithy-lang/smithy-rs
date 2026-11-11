/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.serde

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.customizations.SchemaSerdeAllowlist
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

/** `serde` support for the `DocumentV2` representations that schema-serde clients generate. */
class SerdeDocumentV2Test {
    private val model =
        """
        namespace $SCHEMA_SERDE_NAMESPACE
        use smithy.rust#serde
        use aws.protocols#awsJson1_0

        @awsJson1_0
        service DocService {
            version: "1"
            operations: [Put]
        }

        operation Put {
            input := {
                record: Record
            }
        }

        @serde(serialize: true, deserialize: true)
        structure Record {
            choice: Choice
            payload: Document
            payloads: DocumentList
            payloadMap: DocumentMap
            @deprecated
            legacy: Document
            nested: Nested
        }

        structure Nested {
            @required
            body: Document
        }

        union Choice {
            doc: Document
            docs: DocumentList
            docMap: DocumentMap
            sparseDocs: SparseDocumentList
            secret: SecretDocument
            text: String
        }

        @sensitive
        document SecretDocument

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
        """.asSmithyModel(smithyVersion = "2")

    @Test
    fun `DocumentV2 union variants serialize as their modeled members`() {
        clientIntegrationTest(
            model,
            params =
                IntegrationTestParams(
                    cargoCommand = "cargo test --all-features",
                    service = "$SCHEMA_SERDE_NAMESPACE#DocService",
                ),
        ) { ctx, crate ->
            check(SchemaSerdeAllowlist.usesSchemaSerdeExclusively(ctx)) {
                "this test needs DocumentV2 variants; check that $SCHEMA_SERDE_NAMESPACE is still in SchemaSerdeAllowlist"
            }
            val codegenScope =
                arrayOf(
                    "crate" to RustType.Opaque(ctx.moduleUseName()),
                    "serde_json" to CargoDependency("serde_json", CratesIo("1")).toDevDependency().toType(),
                    "bincode" to CargoDependency("bincode", CratesIo("1")).toDevDependency().toType(),
                    "Document" to RuntimeType.document(ctx.runtimeConfig),
                    "DocumentV2" to RuntimeType.documentV2(ctx.runtimeConfig),
                )

            crate.integrationTest("document_v2_serde") {
                unitTest("union_variants_serialize_like_their_lossy_documents") {
                    rustTemplate(
                        """
                        use #{crate}::serde::{SerializationSettings, SerializeConfigured};
                        use #{crate}::types::{Choice, Record};
                        use #{Document};
                        use #{DocumentV2};
                        use std::collections::HashMap;

                        fn blob() -> DocumentV2 {
                            DocumentV2::Blob(b"hello".to_vec())
                        }

                        fn record(choice: Choice) -> Record {
                            Record::builder().choice(choice).build()
                        }

                        fn json(choice: Choice, settings: &SerializationSettings) -> #{serde_json}::Value {
                            #{serde_json}::to_value(record(choice).serialize_ref(settings))
                                .expect("record should serialize")
                        }

                        fn bincode_bytes(choice: Choice) -> Vec<u8> {
                            #{bincode}::serialize(&record(choice).serialize_ref(&SerializationSettings::default()))
                                .expect("record should serialize")
                        }

                        let lossy = blob().to_document_lossy();
                        let cases = [
                            (Choice::DocV2(blob()), Choice::Doc(lossy.clone())),
                            (
                                Choice::DocsV2(vec![blob(), DocumentV2::Null]),
                                Choice::Docs(vec![lossy.clone(), Document::Null]),
                            ),
                            (
                                Choice::DocMapV2(HashMap::from([("key".to_string(), blob())])),
                                Choice::DocMap(HashMap::from([("key".to_string(), lossy.clone())])),
                            ),
                            (
                                Choice::SparseDocsV2(vec![Some(blob()), None]),
                                Choice::SparseDocs(vec![Some(lossy.clone()), None]),
                            ),
                            (Choice::SecretV2(blob()), Choice::Secret(lossy.clone())),
                        ];
                        let settings = SerializationSettings::leak_sensitive_fields();
                        for (v2, v1) in cases {
                            // A V2 variant writes its modeled member's name, and the value that the
                            // original variant would hold after `to_document_lossy`.
                            let expected = json(v1.clone(), &settings);
                            assert_eq!(json(v2.clone(), &settings), expected, "{v2:?}");
                            // Binary formats write the variant index, which is the modeled member's.
                            assert_eq!(bincode_bytes(v2.clone()), bincode_bytes(v1.clone()), "{v2:?}");
                            // The serde deserializer reads the original variant back.
                            let read: Record = #{serde_json}::from_value(expected)
                                .expect("record should deserialize");
                            assert_eq!(read, record(v1));
                        }

                        assert_eq!(
                            json(Choice::DocV2(blob()), &settings),
                            #{serde_json}::json!({ "choice": { "doc": "aGVsbG8=" } }),
                        );
                        assert_eq!(
                            json(Choice::SecretV2(blob()), &SerializationSettings::redact_sensitive_fields()),
                            #{serde_json}::json!({ "choice": { "secret": "<redacted>" } }),
                        );
                        """,
                        *codegenScope,
                    )
                }

                unitTest("structure_companions_take_priority_over_the_document_fields") {
                    rustTemplate(
                        """
                        ##![allow(deprecated)]
                        use #{crate}::serde::{SerializationSettings, SerializeConfigured};
                        use #{crate}::types::{Nested, Record};
                        use #{Document};
                        use #{DocumentV2};

                        fn blob() -> DocumentV2 {
                            DocumentV2::Blob(b"hello".to_vec())
                        }

                        fn json(record: &Record) -> #{serde_json}::Value {
                            #{serde_json}::to_value(record.serialize_ref(&SerializationSettings::default()))
                                .expect("record should serialize")
                        }

                        let mut record = Record::builder()
                            .payload_v2(blob())
                            .payloads_v2(blob())
                            .payload_map_v2("key", blob())
                            .legacy_v2(blob())
                            .nested(Nested::builder().body_v2(blob()).build().expect("nested should build"))
                            .build();
                        let expected = #{serde_json}::json!({
                            "payload": "aGVsbG8=",
                            "payloads": ["aGVsbG8="],
                            "payloadMap": { "key": "aGVsbG8=" },
                            "legacy": "aGVsbG8=",
                            "nested": { "body": "aGVsbG8=" },
                        });
                        assert_eq!(json(&record), expected);

                        // A set companion is written even after its `Document` field changes, as on the wire.
                        record.payload = Some(Document::from("changed"));
                        record.payloads = Some(vec![]);
                        record.payload_map = None;
                        record.legacy = None;
                        record.nested.as_mut().unwrap().body = Document::Null;
                        assert_eq!(json(&record), expected);

                        // Without a companion, the current `Document` field is written.
                        let mut record = Record::builder().payload(Document::from("before")).build();
                        record.payload = Some(Document::from("after"));
                        assert_eq!(json(&record), #{serde_json}::json!({ "payload": "after" }));
                        """,
                        *codegenScope,
                    )
                }
            }
        }
    }

    companion object {
        private const val SCHEMA_SERDE_NAMESPACE = "smithy.rust.codegen.test.schemaheaders"
    }
}
