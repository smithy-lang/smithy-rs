/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.serde

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.cfg
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.feature
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.runCommand
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class SerdeDecoratorTest {
    private val params =
        IntegrationTestParams(cargoCommand = "cargo test --all-features", service = "com.example#HelloService")
    private val simpleModel =
        """
        namespace com.example
        use smithy.rust#serde
        use aws.protocols#awsJson1_0
        use smithy.framework#ValidationException
        @awsJson1_0
        service HelloService {
            operations: [SayHello, SayGoodBye, Streaming],
            version: "1"
        }
        operation SayHello {
            input: TestInput
            errors: [ValidationException]
        }

        structure Recursive {
            inner: RecursiveList
        }

        list RecursiveList {
            member: Recursive
        }

        @serde(deserialize: true)
        operation Streaming {
            input: StreamingInput
            errors: [ValidationException]
        }

        structure StreamingInput {
            @required
            data: StreamingBlob
        }

        @streaming
        blob StreamingBlob

        @serde(deserialize: true)
        structure TestInput {
           foo: SensitiveString,
           e: TestEnum,
           nested: Nested,
           union: U,
           document: Document,
           blob: SensitiveBlob,
           constrained: Constrained,
           recursive: Recursive,
           map: EnumKeyedMap,
           float: Float,
           double: Double
        }

        structure Constrained {
            shortList: ShortList
            shortMap: ShortMap
            shortBlob: ShortBlob
            rangedInt: RangedInteger,
            rangedLong: RangedLong
        }

        @range(max: 10)
        integer RangedInteger

        @range(max: 10)
        long RangedLong

        @length(max: 10)
        blob ShortBlob

        @length(max: 10)
        map ShortMap {
            key: String,
            value: Nested
        }

        @length(max: 10)
        map EnumKeyedMap {
            key: TestEnum
            value: TestEnum
        }

        @length(max: 10)
        string ShortString

        @length(max: 10)
        list ShortList {
            member: Nested
        }

        @sensitive
        blob SensitiveBlob

        @sensitive
        string SensitiveString

        @sensitive
        enum TestEnum {
            A,
            B,
            C,
            D
        }

        @sensitive
        union U {
            nested: Nested,
            enum: TestEnum,
            other: Unit
        }

        structure Nested {
          @required
          int: Integer,
          float: Float,
          double: Double,
          sensitive: Timestamps,
          notSensitive: AlsoTimestamps,
          manyEnums: TestEnumList,
          sparse: SparseList
          map: SparseMap
        }

        list TestEnumList {
            member: TestEnum
        }

        map Timestamps {
            key: String
            value: SensitiveTimestamp
        }

        map AlsoTimestamps {
            key: String
            value: Timestamp
        }

        @sparse
        map SparseMap {
            key: String
            value: Timestamps
        }

        @sensitive
        timestamp SensitiveTimestamp

        @sparse
        list SparseList {
            member: TestEnum
        }

        operation SayGoodBye {
            input: NotSerde
        }
        structure NotSerde {}
        """.asSmithyModel(smithyVersion = "2")

    @Test
    fun `decorator should traverse resources`() {
        val model =
            """
            namespace com.example
            use smithy.rust#serde
            use aws.protocols#awsJson1_0

            @awsJson1_0
            @serde
            service MyResourceService {
                resources: [MyResource]
            }

            resource MyResource {
                read: ReadMyResource
            }

            @readonly
            operation ReadMyResource {
                input := { }
            }
            """.asSmithyModel(smithyVersion = "2")

        val params =
            IntegrationTestParams(cargoCommand = "cargo test --all-features", service = "com.example#MyResourceService")
        serverIntegrationTest(model, params = params) { ctx, crate ->
            val codegenScope =
                arrayOf(
                    "crate" to RustType.Opaque(ctx.moduleUseName()),
                    "serde_json" to CargoDependency("serde_json", CratesIo("1")).toDevDependency().toType(),
                    // we need the derive feature
                    "serde" to CargoDependency.Serde.toDevDependency().toType(),
                )

            crate.integrationTest("test_serde") {
                unitTest("input_serialized") {
                    rustTemplate(
                        """
                        use #{crate}::input::ReadMyResourceInput;
                        use #{crate}::serde::*;
                        let input = ReadMyResourceInput { };
                        let settings = SerializationSettings::default();
                        let _serialized = #{serde_json}::to_string(&input.serialize_ref(&settings)).expect("failed to serialize");
                        """,
                        *codegenScope,
                    )
                }
            }
        }
    }

    @Test
    fun `feature should not be added if trait is not used`() {
        val model =
            """
            namespace com.example
            use aws.protocols#awsJson1_0

            @awsJson1_0
            service MyService {
                operations: [MyOperation]
            }

            operation MyOperation { }
            """.asSmithyModel(smithyVersion = "2")

        val params =
            IntegrationTestParams(cargoCommand = "cargo test --all-features", service = "com.example#MyService")
        serverIntegrationTest(model, params = params) { _, crate ->
            crate.integrationTest("test_serde") {
                rust("##![allow(unexpected_cfgs)]")
                unitTest("fails_if_serde_feature_exists", additionalAttributes = listOf(Attribute(cfg(feature("serde"))))) {
                    rust("assert!(false);")
                }
            }
        }
    }

    @Test
    fun `serialize and deserialize flags independently control the serde feature`() {
        val serializeOnlyModel =
            """
            namespace com.example
            use smithy.rust#serde
            use aws.protocols#awsJson1_0

            @awsJson1_0
            service SerializeOnlyService {
                operations: [UseSerializeOnly]
            }

            operation UseSerializeOnly {
                input: UseSerializeOnlyInput
            }

            structure UseSerializeOnlyInput {
                value: SerializeOnly
            }

            @serde(serialize: true, deserialize: false)
            structure SerializeOnly {
                value: String
            }
            """.asSmithyModel(smithyVersion = "2")

        clientIntegrationTest(
            serializeOnlyModel,
            params =
                IntegrationTestParams(
                    cargoCommand = "cargo test --all-features",
                    service = "com.example#SerializeOnlyService",
                ),
        ) { ctx, crate ->
            val codegenScope =
                arrayOf(
                    "crate" to RustType.Opaque(ctx.moduleUseName()),
                    "serde_json" to CargoDependency.SerdeJson.toDevDependency().toType(),
                )

            crate.integrationTest("test_serialize_only") {
                unitTest("explicit_serialize_only") {
                    rustTemplate(
                        """
                        use #{crate}::serde::{SerializationSettings, SerializeConfigured};
                        use #{crate}::types::SerializeOnly;

                        let value = SerializeOnly::builder().value("hello").build();
                        let json = #{serde_json}::to_string(
                            &value.serialize_ref(&SerializationSettings::default())
                        ).expect("failed to serialize");
                        assert_eq!(json, r##"{"value":"hello"}"##);
                        """,
                        *codegenScope,
                    )
                }
            }
        }

        val disabledModel =
            """
            namespace com.example
            use smithy.rust#serde
            use aws.protocols#awsJson1_0

            @awsJson1_0
            service DisabledService {
                operations: [Disabled]
            }

            operation Disabled {
                input: DisabledInput
            }

            @serde(serialize: false, deserialize: false)
            structure DisabledInput {}
            """.asSmithyModel(smithyVersion = "2")

        clientIntegrationTest(
            disabledModel,
            params =
                IntegrationTestParams(
                    cargoCommand = "cargo test --all-features",
                    service = "com.example#DisabledService",
                ),
        ) { _, crate ->
            crate.integrationTest("test_disabled_serde") {
                rust("##![allow(unexpected_cfgs)]")
                unitTest(
                    "fails_if_serde_feature_exists",
                    additionalAttributes = listOf(Attribute(cfg(feature("serde")))),
                ) {
                    rust("assert!(false);")
                }
            }
        }
    }

    @Test
    fun `deserialize supports JSON and scoped settings`() {
        val model =
            """
            namespace com.example
            use smithy.rust#serde
            use aws.protocols#awsJson1_0

            @awsJson1_0
            service DeserializationService {
                operations: [ExerciseDeserialization]
            }

            operation ExerciseDeserialization {
                input: ExerciseDeserializationInput
            }

            structure ExerciseDeserializationInput {
                payload: Payload
            }

            @serde(serialize: false, deserialize: true)
            structure Payload {
                message: String
                float: Float
                double: Double
                defaulted: Integer = 7
            }
            """.asSmithyModel(smithyVersion = "2")

        clientIntegrationTest(
            model,
            params =
                IntegrationTestParams(
                    cargoCommand = "cargo test --all-features",
                    service = "com.example#DeserializationService",
                ),
        ) { ctx, crate ->
            val codegenScope =
                arrayOf(
                    "crate" to RustType.Opaque(ctx.moduleUseName()),
                    "serde_json" to CargoDependency.SerdeJson.toDevDependency().toType(),
                    "serde_derive" to
                        CargoDependency("serde_derive", CratesIo("1")).toDevDependency().toType(),
                )

            crate.integrationTest("test_deserialization_api") {
                unitTest("ordinary_json_deserialize") {
                    rustTemplate(
                        """
                        use #{crate}::types::Payload;

                        let value: Payload = #{serde_json}::from_str(
                            r##"{"message":"hello","float":1.5,"double":2.5}"##
                        ).expect("failed to deserialize");
                        assert_eq!(value.message.as_deref(), Some("hello"));
                        assert_eq!(value.float, Some(1.5));
                        assert_eq!(value.double, Some(2.5));
                        assert_eq!(value.defaulted, 7);
                        """,
                        *codegenScope,
                    )
                }

                unitTest("default_and_configured_non_finite_float_strings") {
                    rustTemplate(
                        """
                        use #{crate}::serde::DeserializationSettings;
                        use #{crate}::types::Payload;

                        let json = r##"{"float":"Infinity","double":"NaN"}"##;
                        assert!(#{serde_json}::from_str::<Payload>(json).is_err());

                        let mut settings = DeserializationSettings::default();
                        settings.allow_non_finite_float_strings = true;
                        let value = settings.scope(|| #{serde_json}::from_str::<Payload>(json))
                            .expect("configured non-finite floats should deserialize");
                        assert_eq!(value.float, Some(f32::INFINITY));
                        assert!(value.double.expect("double should be set").is_nan());
                        """,
                        *codegenScope,
                    )
                }

                unitTest("nested_scopes_restore_previous_settings") {
                    rustTemplate(
                        """
                        use #{crate}::serde::DeserializationSettings;
                        use #{crate}::types::Payload;

                        let parses_non_finite = || {
                            #{serde_json}::from_str::<Payload>(
                                r##"{"float":"-Infinity"}"##
                            ).is_ok()
                        };
                        let mut enabled = DeserializationSettings::default();
                        enabled.allow_non_finite_float_strings = true;
                        let disabled = DeserializationSettings::default();

                        assert!(!parses_non_finite());
                        enabled.scope(|| {
                            assert!(parses_non_finite());
                            disabled.scope(|| assert!(!parses_non_finite()));
                            assert!(parses_non_finite());
                        });
                        assert!(!parses_non_finite());
                        """,
                        *codegenScope,
                    )
                }

                unitTest("panicking_scope_restores_previous_settings") {
                    rustTemplate(
                        """
                        use #{crate}::serde::DeserializationSettings;
                        use #{crate}::types::Payload;

                        let mut enabled = DeserializationSettings::default();
                        enabled.allow_non_finite_float_strings = true;
                        let panic = std::panic::catch_unwind(|| {
                            enabled.scope(|| panic!("expected panic"));
                        });
                        assert!(panic.is_err());
                        assert!(#{serde_json}::from_str::<Payload>(
                            r##"{"double":"Infinity"}"##
                        ).is_err());
                        """,
                        *codegenScope,
                    )
                }

                unitTest("scoped_settings_do_not_cross_threads") {
                    rustTemplate(
                        """
                        use #{crate}::serde::DeserializationSettings;
                        use #{crate}::types::Payload;

                        let mut enabled = DeserializationSettings::default();
                        enabled.allow_non_finite_float_strings = true;
                        let child_result = enabled.scope(|| {
                            std::thread::spawn(|| {
                                #{serde_json}::from_str::<Payload>(
                                    r##"{"float":"Infinity"}"##
                                )
                            }).join().expect("child thread should not panic")
                        });
                        assert!(child_result.is_err());
                        """,
                        *codegenScope,
                    )
                }

                unitTest("customer_owned_derive_envelope_uses_scoped_settings") {
                    rustTemplate(
                        """
                        use #{crate}::serde::DeserializationSettings;
                        use #{crate}::types::Payload;

                        ##[derive(#{serde_derive}::Deserialize)]
                        struct Envelope {
                            payload: Payload,
                        }

                        let mut settings = DeserializationSettings::default();
                        settings.allow_non_finite_float_strings = true;
                        let envelope = settings.scope(|| {
                            #{serde_json}::from_str::<Envelope>(
                                r##"{"payload":{"float":"Infinity"}}"##
                            )
                        }).expect("failed to deserialize customer-owned envelope");
                        assert_eq!(envelope.payload.float, Some(f32::INFINITY));
                        """,
                        *codegenScope,
                    )
                }
            }
        }
    }

    val onlyConstrained =
        """
        namespace com.example
        use smithy.rust#serde
        use aws.protocols#awsJson1_0
        use smithy.framework#ValidationException
        @awsJson1_0
        service HelloService {
            operations: [SayHello],
            version: "1"
        }
        @serde
        operation SayHello {
            input: TestInput
            errors: [ValidationException]
        }
        structure TestInput {
            @length(max: 10)
            shortBlob: Blob
        }
        """.asSmithyModel(smithyVersion = "2")

    // There is a "race condition" where if the first blob shape serialized is constrained, it triggered unexpected
    // behavior where the constrained shape was used instead. This test verifies the fix.
    // Fixes https://github.com/smithy-lang/smithy-rs/issues/3890
    @Test
    fun compilesOnlyConstrainedModel() {
        val constrainedShapesSettings =
            Node.objectNodeBuilder().withMember(
                "codegen",
                Node.objectNodeBuilder()
                    .withMember("publicConstrainedTypes", true)
                    .build(),
            ).build()
        serverIntegrationTest(
            onlyConstrained,
            params.copy(additionalSettings = constrainedShapesSettings),
        ) { _codegenContext, _rustCrate ->
        }
    }

    @Test
    fun generateSerializersThatWorkServer() {
        serverIntegrationTest(simpleModel, params = params) { ctx, crate ->
            val codegenScope =
                arrayOf(
                    "crate" to RustType.Opaque(ctx.moduleUseName()),
                    "serde_json" to CargoDependency("serde_json", CratesIo("1")).toDevDependency().toType(),
                    "ciborium" to CargoDependency.Ciborium.toType(),
                    // we need the derive feature
                    "serde" to CargoDependency.Serde.toDevDependency().toType(),
                )

            crate.integrationTest("test_serde") {
                unitTest("input_deserialized_from_json_and_cbor") {
                    rustTemplate(
                        """
                        use #{crate}::input::SayHelloInput;
                        use #{crate}::model::{EnumKeyedMap, Nested, Recursive, TestEnum, U};
                        fn assert_enum_map(map: &EnumKeyedMap) {
                            assert_eq!(map.inner().get(&TestEnum::A), Some(&TestEnum::B));
                        }
                        fn assert_collections(nested: &Nested, recursive: &Recursive) {
                            assert_eq!(
                                nested.many_enums(),
                                Some([TestEnum::A, TestEnum::B].as_slice())
                            );
                            let sparse = nested.sparse().expect("sparse list");
                            assert_eq!(sparse, [None, Some(TestEnum::B)].as_slice());
                            let children = recursive.inner().expect("recursive children");
                            assert_eq!(children.len(), 1);
                            assert!(children[0].inner().is_none());
                        }
                        $roundTripDeserializationTest
                        """,
                        *codegenScope,
                    )
                }

                unitTest("input_serialized") {
                    rustTemplate(
                        """
                        use #{crate}::model::{Nested, U, TestEnum};
                        use #{crate}::input::SayHelloInput;
                        use #{crate}::serde::*;
                        use std::collections::HashMap;
                        use std::time::UNIX_EPOCH;
                        use aws_smithy_types::{DateTime, Document, Blob};
                        let sensitive_map = HashMap::from([("a".to_string(), DateTime::from(UNIX_EPOCH))]);
                        let input = SayHelloInput::builder()
                            .foo(Some("foo-value".to_string()))
                            .e(Some(TestEnum::A))
                            .document(Some(Document::String("hello!".into())))
                            .blob(Some(Blob::new("hello")))
                            .float(Some(f32::INFINITY))
                            .double(Some(f64::NAN))
                            .nested(Some(Nested::builder()
                                .int(5)
                                .float(Some(f32::NEG_INFINITY))
                                .double(Some(f64::NEG_INFINITY))
                                .sensitive(Some(sensitive_map.clone()))
                                .not_sensitive(Some(sensitive_map))
                                .many_enums(Some(vec![TestEnum::A]))
                                .sparse(Some(vec![None, Some(TestEnum::A), Some(TestEnum::B)]))
                                .build().unwrap()
                            ))
                            .union(Some(U::Enum(TestEnum::B)))
                            .build()
                            .unwrap();
                        let mut settings = SerializationSettings::default();
                        settings.out_of_range_floats_as_strings = true;
                        let serialized = #{serde_json}::to_string(&input.serialize_ref(&settings)).expect("failed to serialize");
                        assert_eq!(serialized, ${expectedNoRedactions.dq()});
                        settings.redact_sensitive_fields = true;
                        let serialized = #{serde_json}::to_string(&input.serialize_ref(&settings)).expect("failed to serialize");
                        assert_eq!(serialized, ${expectedRedacted.dq()});
                        """,
                        *codegenScope,
                    )
                }

                unitTest("serde_of_bytestream") {
                    rustTemplate(
                        """
                        use #{crate}::input::StreamingInput;
                        use #{crate}::types::ByteStream;
                        use #{crate}::serde::*;
                        let input = StreamingInput::builder().data(ByteStream::from_static(b"123")).build().unwrap();
                        let settings = SerializationSettings::default();
                        let serialized = #{serde_json}::to_string(&input.serialize_ref(&settings)).expect("failed to serialize");
                        assert_eq!(serialized, ${expectedStreaming.dq()});

                        let deserialized: StreamingInput =
                            #{serde_json}::from_str(&serialized).expect("failed to deserialize JSON");
                        assert_eq!(deserialized.data().bytes(), Some(b"123".as_slice()));

                        let mut serialized = Vec::new();
                        #{ciborium}::ser::into_writer(&input.serialize_ref(&settings), &mut serialized)
                            .expect("failed to serialize CBOR");
                        let deserialized: StreamingInput = #{ciborium}::de::from_reader(serialized.as_slice())
                            .expect("failed to deserialize CBOR");
                        assert_eq!(deserialized.data().bytes(), Some(b"123".as_slice()));
                        """,
                        *codegenScope,
                    )
                }

                unitTest("delegated_serde") {
                    rustTemplate(
                        """
                        use #{crate}::input::SayHelloInput;
                        use #{crate}::serde::*;
                        ##[derive(#{serde}::Serialize)]
                        struct MyRecord {
                            ##[serde(serialize_with = "serialize_redacted")]
                            redact_field: SayHelloInput,
                            ##[serde(serialize_with = "serialize_unredacted")]
                            unredacted_field: SayHelloInput
                        }
                        let input = SayHelloInput::builder().foo(Some("foo-value".to_string())).build().unwrap();

                        let field = MyRecord {
                            redact_field: input.clone(),
                            unredacted_field: input
                        };
                        let serialized = #{serde_json}::to_string(&field).expect("failed to serialize");
                        assert_eq!(serialized, r##"{"redact_field":{"foo":"<redacted>"},"unredacted_field":{"foo":"foo-value"}}"##);
                        """,
                        *codegenScope,
                    )
                }
            }
        }
    }

    private val expectedNoRedactions =
        """{
        "foo": "foo-value",
        "e": "A",
        "nested": {
          "int": 5,
          "float": "-Infinity",
          "double": "-Infinity",
          "sensitive": {
            "a": "1970-01-01T00:00:00Z"
          },
          "notSensitive": {
            "a": "1970-01-01T00:00:00Z"
          },
          "manyEnums": [
            "A"
          ],
          "sparse": [null, "A", "B"]
        },
        "union": {
          "enum": "B"
        },
        "document": "hello!",
        "blob": "aGVsbG8=",
        "float": "Infinity",
        "double": "NaN"
    }""".replace("\\s".toRegex(), "")

    private val expectedRedacted =
        """{
        "foo": "<redacted>",
        "e": "<redacted>",
        "nested": {
          "int": 5,
          "float": "-Infinity",
          "double": "-Infinity",
          "sensitive": {
            "a": "<redacted>"
          },
          "notSensitive": {
            "a": "1970-01-01T00:00:00Z"
          },
          "manyEnums": [
            "<redacted>"
          ],
          "sparse": [null, "<redacted>", "<redacted>"]
        },
        "union": "<redacted>",
        "document": "hello!",
        "blob": "<redacted>",
        "float": "Infinity",
        "double": "NaN"
        }
        """.replace("\\s".toRegex(), "")

    private val expectedStreaming = """{"data":"MTIz"}"""

    private val roundTripDeserializationTest =
        """
        fn assert_input(input: &SayHelloInput) {
            assert_eq!(input.foo(), Some("foo-value"));
            assert_eq!(input.e(), Some(&TestEnum::A));
            assert_eq!(input.float(), Some(1.25));
            assert_eq!(input.double(), Some(2.5));
            assert_eq!(input.blob().expect("blob").as_ref(), b"hello");

            let nested = input.nested().expect("nested structure");
            assert_eq!(nested.int(), 5);
            assert_eq!(
                nested.not_sensitive().expect("timestamp map")["epoch"].secs(),
                0
            );

            let sparse_map = nested.map().expect("sparse map");
            assert!(matches!(sparse_map.get("missing"), Some(None)));
            assert_eq!(
                sparse_map["present"].as_ref().expect("present sparse value")["epoch"].secs(),
                0
            );

            match input.union().expect("union") {
                U::Nested(value) => assert_eq!(value.int(), 7),
                other => panic!("unexpected union variant: {other:?}"),
            }
            assert_eq!(
                input.document()
                    .expect("document")
                    .as_object()
                    .expect("document object")["message"]
                    .as_string(),
                Some("hello")
            );
            assert_enum_map(input.map().expect("enum-keyed map"));

            let recursive = input.recursive().expect("recursive structure");
            assert_collections(nested, recursive);
        }

        let json = r##"{
            "foo":"foo-value",
            "e":"A",
            "nested":{
                "int":5,
                "notSensitive":{"epoch":"1970-01-01T00:00:00Z"},
                "manyEnums":["A","B"],
                "sparse":[null,"B"],
                "map":{
                    "missing":null,
                    "present":{"epoch":"1970-01-01T00:00:00Z"}
                }
            },
            "union":{"nested":{"int":7}},
            "document":{"message":"hello"},
            "blob":"aGVsbG8=",
            "recursive":{"inner":[{}]},
            "map":{"A":"B"},
            "float":1.25,
            "double":2.5
        }"##;

        let from_json: SayHelloInput =
            #{serde_json}::from_str(json).expect("failed to deserialize JSON");
        assert_input(&from_json);

        let cbor_value: #{serde_json}::Value =
            #{serde_json}::from_str(json).expect("failed to construct CBOR value");
        let mut cbor = Vec::new();
        #{ciborium}::ser::into_writer(&cbor_value, &mut cbor).expect("failed to encode CBOR");
        let from_cbor: SayHelloInput = #{ciborium}::de::from_reader(cbor.as_slice())
            .expect("failed to deserialize CBOR");
        assert_input(&from_cbor);
        """

    @Test
    fun generateSerializersThatWorkClient() {
        val path =
            clientIntegrationTest(simpleModel, params = params) { ctx, crate ->
                val codegenScope =
                    arrayOf(
                        "crate" to RustType.Opaque(ctx.moduleUseName()),
                        "serde_json" to CargoDependency("serde_json", CratesIo("1")).toDevDependency().toType(),
                        "ciborium" to CargoDependency.Ciborium.toType(),
                        // we need the derive feature
                        "serde" to CargoDependency.Serde.toDevDependency().toType(),
                    )

                crate.integrationTest("test_serde") {
                    unitTest("input_deserialized_from_json_and_cbor") {
                        rustTemplate(
                            """
                            use #{crate}::operation::say_hello::SayHelloInput;
                            use #{crate}::types::{Nested, Recursive, TestEnum, U};
                            fn assert_enum_map(
                                map: &std::collections::HashMap<TestEnum, TestEnum>
                            ) {
                                assert_eq!(map.get(&TestEnum::A), Some(&TestEnum::B));
                            }
                            fn assert_collections(nested: &Nested, recursive: &Recursive) {
                                assert_eq!(
                                    nested.many_enums(),
                                    [TestEnum::A, TestEnum::B].as_slice()
                                );
                                assert_eq!(
                                    nested.sparse(),
                                    [None, Some(TestEnum::B)].as_slice()
                                );
                                let children = recursive.inner();
                                assert_eq!(children.len(), 1);
                                assert!(children[0].inner().is_empty());
                            }
                            $roundTripDeserializationTest
                            """,
                            *codegenScope,
                        )
                    }

                    unitTest("input_serialized") {
                        rustTemplate(
                            """
                            use #{crate}::types::{Nested, U, TestEnum};
                            use #{crate}::serde::*;
                            use std::time::UNIX_EPOCH;
                            use aws_smithy_types::{DateTime, Document, Blob};
                            let input = #{crate}::operation::say_hello::SayHelloInput::builder()
                                .foo("foo-value")
                                .e("A".into())
                                .document(Document::String("hello!".into()))
                                .blob(Blob::new("hello"))
                                .float(f32::INFINITY)
                                .double(f64::NAN)
                                .nested(Nested::builder()
                                    .int(5)
                                    .float(f32::NEG_INFINITY)
                                    .double(f64::NEG_INFINITY)
                                    .sensitive("a", DateTime::from(UNIX_EPOCH))
                                    .not_sensitive("a", DateTime::from(UNIX_EPOCH))
                                    .many_enums("A".into())
                                    .sparse(None).sparse(Some(TestEnum::A)).sparse(Some(TestEnum::B))
                                    .build().unwrap()
                                )
                                .union(U::Enum("B".into()))
                                .build()
                                .unwrap();
                            let mut settings = #{crate}::serde::SerializationSettings::default();
                            settings.out_of_range_floats_as_strings = true;
                            let serialized = #{serde_json}::to_string(&input.serialize_ref(&settings)).expect("failed to serialize");
                            assert_eq!(serialized, ${expectedNoRedactions.dq()});
                            settings.redact_sensitive_fields = true;
                            let serialized = #{serde_json}::to_string(&input.serialize_ref(&settings)).expect("failed to serialize");
                            assert_eq!(serialized, ${expectedRedacted.dq()});
                            settings.out_of_range_floats_as_strings = false;
                            let serialized = #{serde_json}::to_string(&input.serialize_ref(&settings)).expect("failed to serialize");
                            assert_ne!(serialized, ${expectedRedacted.dq()});
                            """,
                            *codegenScope,
                        )
                    }

                    unitTest("serde_of_bytestream") {
                        rustTemplate(
                            """
                            use #{crate}::operation::streaming::StreamingInput;
                            use #{crate}::primitives::ByteStream;
                            use #{crate}::serde::*;
                            let input = StreamingInput::builder().data(ByteStream::from_static(b"123")).build().unwrap();
                            let settings = SerializationSettings::default();
                            let serialized = #{serde_json}::to_string(&input.serialize_ref(&settings)).expect("failed to serialize");
                            assert_eq!(serialized, ${expectedStreaming.dq()});

                            let deserialized: StreamingInput =
                                #{serde_json}::from_str(&serialized).expect("failed to deserialize JSON");
                            assert_eq!(deserialized.data().bytes(), Some(b"123".as_slice()));
                            assert!(#{serde_json}::from_str::<StreamingInput>(
                                r##"{"data":"streaming data"}"##
                            ).is_err());
                            """,
                            *codegenScope,
                        )
                    }

                    unitTest("delegated_serde") {
                        rustTemplate(
                            """
                            use #{crate}::operation::say_hello::SayHelloInput;
                            use #{crate}::serde::*;
                            ##[derive(#{serde}::Serialize)]
                            struct MyRecord {
                                ##[serde(serialize_with = "serialize_redacted")]
                                redact_field: SayHelloInput,
                                ##[serde(serialize_with = "serialize_unredacted")]
                                unredacted_field: SayHelloInput
                            }
                            let input = SayHelloInput::builder().foo("foo-value").build().unwrap();

                            let field = MyRecord {
                                redact_field: input.clone(),
                                unredacted_field: input
                            };
                            let serialized = #{serde_json}::to_string(&field).expect("failed to serialize");
                            assert_eq!(serialized, r##"{"redact_field":{"foo":"<redacted>"},"unredacted_field":{"foo":"foo-value"}}"##);
                            """,
                            *codegenScope,
                        )
                    }

                    unitTest("cbor") {
                        rustTemplate(
                            """
                            use #{crate}::operation::streaming::StreamingInput;
                            use #{crate}::primitives::ByteStream;
                            use #{crate}::serde::*;
                            let input = StreamingInput::builder().data(ByteStream::from_static(b"123")).build().unwrap();
                            let settings = SerializationSettings::default();
                            let mut serialized = Vec::new();
                            #{ciborium}::ser::into_writer(&input.serialize_ref(&settings), &mut serialized)
                                .expect("failed to serialize input into CBOR format using `ciborium`");
                            assert_eq!(serialized, b"\xa1ddataC123");

                            let deserialized: StreamingInput =
                                #{ciborium}::de::from_reader(serialized.as_slice())
                                    .expect("failed to deserialize CBOR");
                            assert_eq!(deserialized.data().bytes(), Some(b"123".as_slice()));
                            """,
                            *codegenScope,
                        )
                    }
                }
            }
        "cargo clippy --all-features".runCommand(path)
    }
}
