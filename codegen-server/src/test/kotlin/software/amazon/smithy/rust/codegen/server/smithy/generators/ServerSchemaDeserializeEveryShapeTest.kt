/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class ServerSchemaDeserializeEveryShapeTest {
    private val model =
        """
        ${'$'}version: "2"
        namespace test
        use aws.protocols#restJson1
        use smithy.framework#ValidationException

        @restJson1
        service EveryShapeService {
            version: "1",
            operations: [Op],
        }

        @http(method: "POST", uri: "/op")
        operation Op {
            input: OpInput,
            output: OpOutput,
            errors: [ValidationException],
        }

        structure OpInput {
            shared: Shared,
            choice: SharedChoice,
        }

        structure OpOutput {
            message: Message,
        }

        structure Message {
            @required
            text: String,
            count: Integer,
            kind: Kind,
            title: Title,
            label: Label,
            nested: Nested,
            shared: Shared,
            kinds: KindList,
            tags: TagList,
            byKind: KindMap,
            sparse: SparseKindList,
            choice: Choice,
            sharedChoice: SharedChoice,
            next: Message,
        }

        structure Nested {
            flag: Boolean,
        }

        structure Shared {
            @required
            id: Title,
            kinds: KindList,
        }

        @length(min: 1, max: 5)
        string Title

        @length(max: 3)
        string Label

        enum Kind { A, B }

        list KindList { member: Kind }

        list TagList { member: String }

        map KindMap { key: Kind, value: Integer }

        @sparse
        list SparseKindList { member: Kind }

        union Choice {
            text: String,
            flag: Boolean,
        }

        union SharedChoice {
            name: Title,
            flag: Boolean,
        }
        """.asSmithyModel()

    private fun run(publicConstrainedTypes: Boolean) =
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                service = "test#EveryShapeService",
                additionalSettings =
                    ServerAdditionalSettings.builder()
                        .schemaSerde(true)
                        .publicConstrainedTypes(publicConstrainedTypes)
                        .toObjectNode(),
            ),
            testCoverage = HttpTestType.Default,
        ) { ctx, rustCrate ->
            val json = RuntimeType.smithyJson(ctx.runtimeConfig)
            rustCrate.testModule {
                unitTest("output_only_shapes_read_into_public_types") {
                    rustTemplate(
                        """
                        use crate::model::*;
                        use #{Codec};
                        let codec = #{JsonCodec}::new(#{JsonCodecSettings}::default());
                        let rejected = |body: &[u8]| {
                            let mut deser = codec.create_deserializer(body);
                            Message::deserialize_schema(&mut deser).is_err()
                        };
                        let body = br##"{"text":"hi","count":3,"kind":"A","title":"ok","label":"abc",
                            "nested":{"flag":true},"shared":{"id":"x","kinds":["B"]},"kinds":["A","B"],"tags":["t"],
                            "byKind":{"A":1},"sparse":["A",null],"choice":{"flag":true},"sharedChoice":{"name":"n"},
                            "next":{"text":"inner"}}"##;
                        let mut deser = codec.create_deserializer(body);
                        let message = Message::deserialize_schema(&mut deser).unwrap();
                        assert_eq!(message.text, "hi");
                        assert_eq!(message.count, Some(3));
                        assert_eq!(message.kind, Some(Kind::A));
                        assert_eq!(message.nested, Some(Nested { flag: Some(true) }));
                        assert_eq!(message.kinds, Some(vec![Kind::A, Kind::B]));
                        assert_eq!(message.tags, Some(vec!["t".to_owned()]));
                        assert_eq!(message.by_kind.as_ref().and_then(|m| m.get(&Kind::A)), Some(&1));
                        assert_eq!(message.sparse, Some(vec![Some(Kind::A), None]));
                        assert!(matches!(message.choice, Some(Choice::Flag(true))));
                        assert!(matches!(message.shared_choice, Some(SharedChoice::Name(_))));
                        assert_eq!(message.next.as_ref().unwrap().text, "inner");
                        #{constrainedAsserts:W}

                        assert!(rejected(br##"{"text":"hi","kind":"Z"}"##), "unknown closed enum value");
                        assert!(rejected(br##"{"text":"hi","kinds":["Z"]}"##), "unknown enum in a list");
                        assert!(rejected(br##"{"text":"hi","byKind":{"Z":1}}"##), "unknown enum key");
                        assert!(rejected(br##"{"count":1}"##), "missing required member");
                        assert!(rejected(br##"{"text":"hi","shared":{"id":"far too long"}}"##), "an input shape still validates its constraints");
                        assert!(rejected(br##"{"text":"hi","shared":{"kinds":[]}}"##), "an input shape still validates required members");
                        assert!(rejected(br##"{"text":"hi","sharedChoice":{"name":"far too long"}}"##), "an input union still validates its constraints");
                        """,
                        "Codec" to RuntimeType.smithySchema(ctx.runtimeConfig).resolve("codec::Codec"),
                        "JsonCodec" to json.resolve("codec::JsonCodec"),
                        "JsonCodecSettings" to json.resolve("codec::JsonCodecSettings"),
                        "constrainedAsserts" to
                            writable {
                                if (publicConstrainedTypes) {
                                    rust(
                                        """
                                        assert_eq!(message.title.as_ref().map(|t| t.as_str()), Some("ok"));
                                        assert_eq!(message.label.as_ref().map(|l| l.as_str()), Some("abc"));
                                        assert_eq!(message.shared.as_ref().unwrap().id.as_str(), "x");
                                        assert!(rejected(br##"{"text":"hi","title":"far too long"}"##), "a public constrained newtype rejects on conversion");
                                        assert!(rejected(br##"{"text":"hi","label":"long"}"##), "a public constrained newtype rejects on conversion");
                                        """,
                                    )
                                } else {
                                    rust(
                                        """
                                        assert_eq!(message.title.as_deref(), Some("ok"));
                                        assert_eq!(message.label.as_deref(), Some("abc"));
                                        assert_eq!(message.shared.as_ref().unwrap().id, "x");
                                        assert!(!rejected(br##"{"text":"hi","title":"far too long"}"##), "an output-only read of a plain String does not validate");
                                        assert!(!rejected(br##"{"text":"hi","label":"long"}"##), "an output-only read of a plain String does not validate");
                                        """,
                                    )
                                }
                            },
                    )
                }
            }
        }

    @Test
    fun `every shape gets a walker with public constrained types`() {
        run(publicConstrainedTypes = true)
    }

    @Test
    fun `every shape gets a walker without public constrained types`() {
        run(publicConstrainedTypes = false)
    }
}
