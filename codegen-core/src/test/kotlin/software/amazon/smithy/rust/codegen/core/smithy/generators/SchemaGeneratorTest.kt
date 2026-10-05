/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import io.kotest.matchers.string.shouldContain
import io.kotest.matchers.string.shouldNotContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.implBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.testCodegenContext
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup

class SchemaGeneratorTest {
    private val model =
        """
        namespace test

        structure MyStruct {
            name: String,
            age: Integer,
            active: Boolean
        }

        structure ComplexStruct {
            label: String,
            count: Long,
            ratio: Double,
            enabled: Boolean,
            data: Blob,
            created_at: Timestamp,
            nested: MyStruct,
            tags: TagList,
            metadata: StringMap
        }

        list TagList {
            member: String
        }

        map StringMap {
            key: String,
            value: String
        }

        union MyUnion {
            stringVariant: String,
            intVariant: Integer,
            unitVariant: Unit
        }

        union UnitUnion {
            unit: Unit
        }

        structure NestedAggregates {
            structMap: StructMap,
            unionList: UnionList,
            mapOfMaps: MapOfMaps
        }

        map StructMap {
            key: String,
            value: MyStruct
        }

        list UnionList {
            member: MyUnion
        }

        map MapOfMaps {
            key: String,
            value: StringMap
        }

        structure CollectionHelperStruct {
            stringList: StringList,
            blobList: BlobList,
            intList: IntList,
            longList: LongList,
            stringStringMap: StringMap
        }

        list StringList {
            member: String
        }

        list BlobList {
            member: Blob
        }

        list IntList {
            member: Integer
        }

        list LongList {
            member: Long
        }

        structure SparseNestedAggregates {
            listOfSparseLists: ListOfSparseStringList,
            mapOfSparseLists: MapOfSparseStringList,
            sparseListOfSparseLists: SparseListOfSparseStringList,
            listOfSparseMaps: ListOfSparseStringMap,
            mapOfSparseMaps: MapOfSparseStringMap
        }

        list ListOfSparseStringList {
            member: SparseStringList
        }

        @sparse
        list SparseStringList {
            member: String
        }

        map MapOfSparseStringList {
            key: String,
            value: SparseStringList
        }

        @sparse
        list SparseListOfSparseStringList {
            member: SparseStringList
        }

        list ListOfSparseStringMap {
            member: SparseStringMap
        }

        @sparse
        map SparseStringMap {
            key: String,
            value: String
        }

        map MapOfSparseStringMap {
            key: String,
            value: SparseStringMap
        }
        """.asSmithyModel()

    private val provider = testSymbolProvider(model)
    private val codegenContext = testCodegenContext(model)

    /** Renders a structure, its builder, and its schema into the given writer. */
    private fun renderStructWithSchema(
        writer: software.amazon.smithy.rust.codegen.core.rustlang.RustWriter,
        testModel: software.amazon.smithy.model.Model,
        testProvider: software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider,
        testContext: software.amazon.smithy.rust.codegen.core.smithy.CodegenContext,
        shape: StructureShape,
        project: software.amazon.smithy.rust.codegen.core.testutil.TestWriterDelegator,
        traitFilter: SchemaTraitFilter = SchemaTraitFilter(testModel),
    ) {
        StructureGenerator(
            testModel, testProvider, writer, shape, emptyList(),
            StructSettings(flattenVecAccessors = true),
        ).render()
        writer.implBlock(testProvider.toSymbol(shape)) {
            BuilderGenerator.renderConvenienceMethod(this, testProvider, shape)
        }
        project.withModule(testProvider.moduleForBuilder(shape)) {
            BuilderGenerator(testModel, testProvider, shape, emptyList()).render(this)
        }
        SchemaGenerator(testContext, writer, shape, traitFilter).render()
    }

    @Test
    fun `schema for structure compiles and works at runtime`() {
        val project = TestWorkspace.testProject(provider)
        val shape = model.lookup<StructureShape>("test#MyStruct")
        project.useShapeWriter(shape) {
            renderStructWithSchema(this, model, provider, codegenContext, shape, project)
            unitTest(
                "schema_structure",
                """
                use aws_smithy_schema::Schema;
                use aws_smithy_schema::serde::SerializableStruct;
                let value = MyStruct::builder().build();
                let erased: &dyn SerializableStruct = &value;
                assert!(std::ptr::eq(erased.schema(), MyStruct::SCHEMA));
                let schema = MyStruct::SCHEMA;
                assert_eq!(schema.shape_type(), aws_smithy_schema::ShapeType::Structure);
                assert_eq!(schema.shape_id().as_str(), "test#MyStruct");
                // member lookup by name
                assert!(schema.member_schema("name").is_some());
                assert!(schema.member_schema("age").is_some());
                assert!(schema.member_schema("active").is_some());
                assert!(schema.member_schema("nonexistent").is_none());
                // member lookup by index
                let m = schema.member_schema_by_index(0).expect("index 0");
                assert_eq!(m.member_name(), Some("name"));
                // members slice
                let names: Vec<&str> = schema.members().iter().filter_map(|m| m.member_name()).collect();
                assert_eq!(names, vec!["name", "age", "active"]);
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `member schemas have correct target types`() {
        val project = TestWorkspace.testProject(provider)
        val shape = model.lookup<StructureShape>("test#MyStruct")
        project.useShapeWriter(shape) {
            renderStructWithSchema(this, model, provider, codegenContext, shape, project)
            unitTest(
                "member_schema_types",
                """
                use aws_smithy_schema::{Schema, ShapeType};
                let schema = MyStruct::SCHEMA;
                let name_schema = schema.member_schema("name").unwrap();
                assert_eq!(name_schema.shape_type(), ShapeType::String);
                assert_eq!(name_schema.member_name(), Some("name"));
                assert_eq!(name_schema.member_index(), Some(0));

                let age_schema = schema.member_schema("age").unwrap();
                assert_eq!(age_schema.shape_type(), ShapeType::Integer);
                assert_eq!(age_schema.member_index(), Some(1));

                let active_schema = schema.member_schema("active").unwrap();
                assert_eq!(active_schema.shape_type(), ShapeType::Boolean);
                assert_eq!(active_schema.member_index(), Some(2));
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `schema for complex structure with nested types compiles`() {
        val project = TestWorkspace.testProject(provider)
        val myStruct = model.lookup<StructureShape>("test#MyStruct")
        val complexStruct = model.lookup<StructureShape>("test#ComplexStruct")
        project.useShapeWriter(myStruct) {
            renderStructWithSchema(this, model, provider, codegenContext, myStruct, project)
        }
        project.useShapeWriter(complexStruct) {
            renderStructWithSchema(this, model, provider, codegenContext, complexStruct, project)
            unitTest(
                "complex_schema",
                """
                use aws_smithy_schema::{Schema, ShapeType};
                let s = ComplexStruct::SCHEMA;
                assert_eq!(s.shape_type(), ShapeType::Structure);
                assert_eq!(s.shape_id().as_str(), "test#ComplexStruct");

                // Primitive member types
                assert_eq!(s.member_schema("label").unwrap().shape_type(), ShapeType::String);
                assert_eq!(s.member_schema("count").unwrap().shape_type(), ShapeType::Long);
                assert_eq!(s.member_schema("ratio").unwrap().shape_type(), ShapeType::Double);
                assert_eq!(s.member_schema("enabled").unwrap().shape_type(), ShapeType::Boolean);
                assert_eq!(s.member_schema("data").unwrap().shape_type(), ShapeType::Blob);
                assert_eq!(s.member_schema("created_at").unwrap().shape_type(), ShapeType::Timestamp);

                // Nested structure member
                assert_eq!(s.member_schema("nested").unwrap().shape_type(), ShapeType::Structure);

                // List member
                assert_eq!(s.member_schema("tags").unwrap().shape_type(), ShapeType::List);

                // Map member
                assert_eq!(s.member_schema("metadata").unwrap().shape_type(), ShapeType::Map);

                // All 9 members present via slice
                let names: Vec<&str> = s.members().iter().filter_map(|m| m.member_name()).collect();
                assert_eq!(names.len(), 9);

                // Index-based access consistent with members order
                for (i, member) in s.members().iter().enumerate() {
                    let by_idx = s.member_schema_by_index(i).unwrap();
                    assert_eq!(member.member_name(), by_idx.member_name());
                }
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `schema for union compiles`() {
        val project = TestWorkspace.testProject(provider)
        val shape = model.lookup<UnionShape>("test#MyUnion")
        project.useShapeWriter(shape) {
            UnionGenerator(model, provider, this, shape).render()
            SchemaGenerator(codegenContext, this, shape).render()
            unitTest(
                "schema_union",
                """
                use aws_smithy_schema::Schema;
                use aws_smithy_schema::serde::SerializableStruct;
                let value = MyUnion::StringVariant("value".into());
                let erased: &dyn SerializableStruct = &value;
                assert!(std::ptr::eq(erased.schema(), MyUnion::SCHEMA));
                let schema = MyUnion::SCHEMA;
                assert_eq!(schema.shape_type(), aws_smithy_schema::ShapeType::Union);
                assert!(schema.member_schema("stringVariant").is_some());
                assert!(schema.member_schema("intVariant").is_some());
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `unit union serialization uses member target schemas`() {
        val project = TestWorkspace.testProject(provider)
        val shape = model.lookup<UnionShape>("test#UnitUnion")
        project.useShapeWriter(shape) {
            UnionGenerator(model, provider, this, shape).render()
            SchemaGenerator(codegenContext, this, shape).render()
            rustTemplate(
                "use #{JsonCodec};",
                "JsonCodec" to RuntimeType.smithyJson(codegenContext.runtimeConfig).resolve("codec::JsonCodec"),
            )
            unitTest(
                "unit_union_schema_and_serialization",
                """
                use aws_smithy_schema::serde::{SerializableStruct, ShapeSerializer};
                use aws_smithy_schema::codec::Codec;

                let value = UnitUnion::Unit;
                let erased: &dyn SerializableStruct = &value;
                let codec = JsonCodec::default();
                let mut ser = codec.create_serializer();
                ser.write_struct(UnitUnion::SCHEMA, erased).unwrap();
                assert_eq!(String::from_utf8(ser.finish()).unwrap(), r#"{"unit":{}}"#);
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `union SerializableStruct impl serializes variants`() {
        val project = TestWorkspace.testProject(provider)
        val shape = model.lookup<UnionShape>("test#MyUnion")
        project.useShapeWriter(shape) {
            UnionGenerator(model, provider, this, shape).render()
            SchemaGenerator(codegenContext, this, shape).render()
            rustTemplate(
                "use #{JsonCodec};",
                "JsonCodec" to RuntimeType.smithyJson(codegenContext.runtimeConfig).resolve("codec::JsonCodec"),
            )
            unitTest(
                "union_serializable_struct_string",
                """
                use aws_smithy_schema::serde::{SerializableStruct, ShapeSerializer};
                use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
                use aws_smithy_schema::codec::Codec;

                let val_str = MyUnion::StringVariant("hello".to_string());
                let codec = JsonCodec::new(JsonCodecSettings::default());
                let mut ser = codec.create_serializer();
                ser.write_struct(MyUnion::SCHEMA, &val_str).expect("serialization should succeed");
                let json = String::from_utf8(ser.finish()).unwrap();
                assert_eq!(json, r#"{"stringVariant":"hello"}"#);
                """,
            )
            unitTest(
                "union_serializable_struct_unit",
                """
                use aws_smithy_schema::serde::{SerializableStruct, ShapeSerializer};
                use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
                use aws_smithy_schema::codec::Codec;

                let value = MyUnion::UnitVariant;
                let erased: &dyn SerializableStruct = &value;
                assert!(std::ptr::eq(erased.schema(), MyUnion::SCHEMA));
                let codec = JsonCodec::new(JsonCodecSettings::default());
                let mut ser = codec.create_serializer();
                ser.write_struct(erased.schema(), erased).expect("unit serialization should succeed");
                assert_eq!(String::from_utf8(ser.finish()).unwrap(), r#"{"unitVariant":{}}"#);
                """,
            )
            unitTest(
                "union_serializable_struct_int",
                """
                use aws_smithy_schema::serde::{SerializableStruct, ShapeSerializer};
                use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
                use aws_smithy_schema::codec::Codec;

                let val_int = MyUnion::IntVariant(42);
                let codec = JsonCodec::new(JsonCodecSettings::default());
                let mut ser = codec.create_serializer();
                ser.write_struct(MyUnion::SCHEMA, &val_int).expect("serialization should succeed");
                let json = String::from_utf8(ser.finish()).unwrap();
                assert_eq!(json, r#"{"intVariant":42}"#);
                """,
            )
            unitTest(
                "union_deserialize_string",
                """
                use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
                use aws_smithy_schema::codec::Codec;

                let json = br#"{"stringVariant":"hello"}"#;
                let codec = JsonCodec::new(JsonCodecSettings::default());
                let mut deser = codec.create_deserializer(json);
                let result = MyUnion::deserialize(&mut deser).expect("deserialization should succeed");
                assert!(matches!(result, MyUnion::StringVariant(ref s) if s == "hello"));
                """,
            )
            unitTest(
                "union_deserialize_int",
                """
                use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
                use aws_smithy_schema::codec::Codec;

                let json = br#"{"intVariant":42}"#;
                let codec = JsonCodec::new(JsonCodecSettings::default());
                let mut deser = codec.create_deserializer(json);
                let result = MyUnion::deserialize(&mut deser).expect("deserialization should succeed");
                assert!(matches!(result, MyUnion::IntVariant(42)));
                """,
            )
            unitTest(
                "union_round_trip",
                """
                use aws_smithy_schema::serde::{SerializableStruct, ShapeSerializer};
                use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
                use aws_smithy_schema::codec::Codec;

                let original = MyUnion::StringVariant("round-trip".to_string());
                let codec = JsonCodec::new(JsonCodecSettings::default());
                let mut ser = codec.create_serializer();
                ser.write_struct(MyUnion::SCHEMA, &original).expect("serialization");
                let bytes = ser.finish();
                let mut deser = codec.create_deserializer(&bytes);
                let result = MyUnion::deserialize(&mut deser).expect("deserialization");
                assert!(matches!(result, MyUnion::StringVariant(ref s) if s == "round-trip"));
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `client union deserialize rejects mixed variants and keeps unknown keys as the unknown variant`() {
        val project = TestWorkspace.testProject(provider)
        val shape = model.lookup<UnionShape>("test#MyUnion")
        project.useShapeWriter(shape) {
            UnionGenerator(model, provider, this, shape).render()
            SchemaGenerator(codegenContext, this, shape).render()
            rustTemplate(
                "use #{JsonCodec};",
                "JsonCodec" to RuntimeType.smithyJson(codegenContext.runtimeConfig).resolve("codec::JsonCodec"),
            )
            unitTest(
                "client_union_unknown_and_mixed",
                """
                use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
                use aws_smithy_schema::codec::Codec;

                let codec = JsonCodec::new(JsonCodecSettings::default());
                let read = |json: &[u8]| MyUnion::deserialize(&mut codec.create_deserializer(json));

                // An unknown key is the unknown variant; its value is skipped.
                assert!(matches!(read(br#"{"zzz":{"nested":[1,2]}}"#).unwrap(), MyUnion::Unknown));
                // `__type` is a discriminator, not a variant.
                assert!(matches!(read(br#"{"__type":"test#MyUnion","intVariant":42}"#).unwrap(), MyUnion::IntVariant(42)));
                // A second key of any kind is an error.
                let err = read(br#"{"stringVariant":"hello","intVariant":1}"#).unwrap_err();
                assert!(err.to_string().contains("mixed variants"), "{err}");
                let err = read(br#"{"intVariant":1,"zzz":true}"#).unwrap_err();
                assert!(err.to_string().contains("mixed variants"), "{err}");
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `server union deserialize rejects unknown keys and mixed variants`() {
        val serverContext = testCodegenContext(model, codegenTarget = CodegenTarget.SERVER)
        val project = TestWorkspace.testProject(provider)
        val shape = model.lookup<UnionShape>("test#MyUnion")
        project.useShapeWriter(shape) {
            UnionGenerator(model, provider, this, shape, renderUnknownVariant = false).render()
            SchemaGenerator(serverContext, this, shape).render()
            rustTemplate(
                "use #{JsonCodec};",
                "JsonCodec" to RuntimeType.smithyJson(serverContext.runtimeConfig).resolve("codec::JsonCodec"),
            )
            unitTest(
                "server_union_unknown_and_mixed",
                """
                use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
                use aws_smithy_schema::codec::Codec;

                let codec = JsonCodec::new(JsonCodecSettings::default());
                let read = |json: &[u8]| MyUnion::deserialize(&mut codec.create_deserializer(json));

                assert!(matches!(read(br#"{"intVariant":42}"#).unwrap(), MyUnion::IntVariant(42)));
                // An unknown key is an error; its value is skipped.
                let err = read(br#"{"zzz":{"nested":[1,2]}}"#).unwrap_err();
                assert!(err.to_string().contains("unexpected union variant"), "{err}");
                // Known plus unknown, and two known members, are both mixed variants.
                let err = read(br#"{"intVariant":1,"zzz":true}"#).unwrap_err();
                assert!(err.to_string().contains("mixed variants"), "{err}");
                let err = read(br#"{"stringVariant":"hello","intVariant":1}"#).unwrap_err();
                assert!(err.to_string().contains("mixed variants"), "{err}");
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `SerializableStruct impl compiles and serializes members`() {
        val project = TestWorkspace.testProject(provider)
        val shape = model.lookup<StructureShape>("test#MyStruct")
        project.useShapeWriter(shape) {
            renderStructWithSchema(this, model, provider, codegenContext, shape, project)
            // Reference JsonCodec via rustTemplate to auto-add the aws-smithy-json dependency
            rustTemplate(
                "use #{JsonCodec};",
                "JsonCodec" to RuntimeType.smithyJson(codegenContext.runtimeConfig).resolve("codec::JsonCodec"),
            )
            unitTest(
                "serializable_struct",
                """
                use aws_smithy_schema::serde::SerializableStruct;
                let s = MyStruct { name: Some("Alice".to_string()), age: Some(30), active: Some(true) };
                fn assert_serializable<T: SerializableStruct>(_t: &T) {}
                assert_serializable(&s);
                """,
            )
            unitTest(
                "serializable_struct_json_output",
                """
                use aws_smithy_schema::serde::{SerializableStruct, ShapeSerializer};
                use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
                use aws_smithy_schema::codec::Codec;

                let s = MyStruct { name: Some("Alice".to_string()), age: Some(30), active: Some(true) };
                let codec = JsonCodec::new(JsonCodecSettings::default());
                let mut ser = codec.create_serializer();
                ser.write_struct(MyStruct::SCHEMA, &s).expect("serialization should succeed");
                let bytes = ser.finish();
                let json = String::from_utf8(bytes).unwrap();
                assert_eq!(json, r#"{"name":"Alice","age":30,"active":true}"#);
                """,
            )
            unitTest(
                "serializable_struct_json_partial",
                """
                use aws_smithy_schema::serde::{SerializableStruct, ShapeSerializer};
                use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
                use aws_smithy_schema::codec::Codec;

                // Only some fields set — None fields should be omitted
                let s = MyStruct { name: Some("Bob".to_string()), age: None, active: None };
                let codec = JsonCodec::new(JsonCodecSettings::default());
                let mut ser = codec.create_serializer();
                ser.write_struct(MyStruct::SCHEMA, &s).expect("serialization should succeed");
                let bytes = ser.finish();
                let json = String::from_utf8(bytes).unwrap();
                assert_eq!(json, r#"{"name":"Bob"}"#);
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `deserialize method works with JsonCodec`() {
        val project = TestWorkspace.testProject(provider)
        val shape = model.lookup<StructureShape>("test#MyStruct")
        project.useShapeWriter(shape) {
            renderStructWithSchema(this, model, provider, codegenContext, shape, project)
            // Add aws-smithy-json dependency
            rustTemplate(
                "use #{JsonCodec};",
                "JsonCodec" to RuntimeType.smithyJson(codegenContext.runtimeConfig).resolve("codec::JsonCodec"),
            )
            unitTest(
                "deserialize_from_json",
                """
                use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
                use aws_smithy_schema::codec::Codec;

                let json = br#"{"name":"Alice","age":30,"active":true}"#;
                let codec = JsonCodec::new(JsonCodecSettings::default());
                let mut deser = codec.create_deserializer(json);
                let result = MyStruct::deserialize(&mut deser).expect("deserialization should succeed");
                assert_eq!(result.name, Some("Alice".to_string()));
                assert_eq!(result.age, Some(30));
                assert_eq!(result.active, Some(true));
                """,
            )
            unitTest(
                "deserialize_partial_json",
                """
                use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
                use aws_smithy_schema::codec::Codec;

                let json = br#"{"name":"Bob"}"#;
                let codec = JsonCodec::new(JsonCodecSettings::default());
                let mut deser = codec.create_deserializer(json);
                let result = MyStruct::deserialize(&mut deser).expect("deserialization should succeed");
                assert_eq!(result.name, Some("Bob".to_string()));
                assert_eq!(result.age, None);
                assert_eq!(result.active, None);
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `trait filtering includes sensitive and jsonName`() {
        val traitModel =
            """
            namespace test
            @sensitive
            structure SecretData {
                @jsonName("user_name")
                name: String,
                password: String,
                @deprecated
                oldField: String
            }
            """.asSmithyModel()

        val traitProvider = testSymbolProvider(traitModel)
        val traitContext = testCodegenContext(traitModel)
        val project = TestWorkspace.testProject(traitProvider)
        val shape = traitModel.lookup<StructureShape>("test#SecretData")
        project.useShapeWriter(shape) {
            renderStructWithSchema(this, traitModel, traitProvider, traitContext, shape, project)
            unitTest(
                "trait_filtering",
                """
                use aws_smithy_schema::traits::SensitiveTrait;
                let s = SecretData::SCHEMA;

                // @sensitive is included as a direct field
                assert!(s.sensitive().is_some(), "should include @sensitive");

                // @jsonName is on the member schema, not the struct
                let name_member = s.member_schema("name").expect("should have name member");
                assert_eq!(name_member.json_name().map(|j| j.value()), Some("user_name"));

                // password has no jsonName
                let pw_member = s.member_schema("password").expect("should have password member");
                assert!(pw_member.json_name().is_none());
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `unknown traits stored in fallback TraitMap`() {
        val customTraitModel =
            """
            namespace test

            @trait(selector: "structure")
            structure myCustomTrait {
                setting: String
            }

            @trait(selector: "structure")
            structure myAnnotationCustomTrait {}

            @trait(selector: "structure")
            structure myComplexTrait {
                items: ComplexItems,
                count: Integer,
                enabled: Boolean,
                nested: NestedSetting
            }

            list ComplexItems {
                member: String
            }

            structure NestedSetting {
                inner: String
            }

            @myCustomTrait(setting: "hello")
            @myAnnotationCustomTrait
            @myComplexTrait(items: ["a", "b"], count: 3, enabled: true, nested: { inner: "deep" })
            structure Tagged {
                value: String
            }
            """.asSmithyModel()

        val customProvider = testSymbolProvider(customTraitModel)
        val customContext = testCodegenContext(customTraitModel)
        val filter =
            SchemaTraitFilter(
                customTraitModel,
                setOf(
                    software.amazon.smithy.model.shapes.ShapeId.from("test#myCustomTrait"),
                    software.amazon.smithy.model.shapes.ShapeId.from("test#myAnnotationCustomTrait"),
                    software.amazon.smithy.model.shapes.ShapeId.from("test#myComplexTrait"),
                ),
            )
        val project = TestWorkspace.testProject(customProvider)
        val shape = customTraitModel.lookup<StructureShape>("test#Tagged")
        project.useShapeWriter(shape) {
            renderStructWithSchema(this, customTraitModel, customProvider, customContext, shape, project, filter)
            unitTest(
                "unknown_traits",
                """
                use aws_smithy_schema::{DocumentTrait, Trait};
                use aws_smithy_types::Document;
                let s = Tagged::SCHEMA;

                // Unknown traits are stored in the fallback TraitMap
                let traits = s.traits().expect("should have a fallback trait map");

                // A structured (object-valued) custom trait is stored as a DocumentTrait
                // whose value preserves the structure (NOT flattened to a JSON string).
                let custom_id = aws_smithy_schema::shape_id!("test", "myCustomTrait");
                let custom = traits.get(&custom_id).expect("should include custom trait");
                let doc_trait = custom.as_any().downcast_ref::<DocumentTrait>()
                    .expect("unknown complex trait should be a DocumentTrait");
                match doc_trait.value() {
                    Document::Object(obj) => match obj.get("setting") {
                        Some(Document::String(s)) => assert_eq!(s, "hello"),
                        other => panic!("expected setting=String(hello), got: {other:?}"),
                    },
                    other => panic!("expected Document::Object, got: {other:?}"),
                }

                // A richly-structured custom trait round-trips arrays, numbers, bools,
                // and nested objects as structured Document values.
                let complex_id = aws_smithy_schema::shape_id!("test", "myComplexTrait");
                let complex = traits.get(&complex_id).expect("should include complex trait");
                let complex_doc = complex.as_any().downcast_ref::<DocumentTrait>()
                    .expect("complex trait should be a DocumentTrait");
                let obj = match complex_doc.value() {
                    Document::Object(obj) => obj,
                    other => panic!("expected Document::Object, got: {other:?}"),
                };
                match obj.get("items") {
                    Some(Document::Array(items)) => {
                        let strings: Vec<&str> = items.iter().map(|d| match d {
                            Document::String(s) => s.as_str(),
                            other => panic!("expected Document::String element, got: {other:?}"),
                        }).collect();
                        assert_eq!(strings, vec!["a", "b"]);
                    }
                    other => panic!("expected items=Array, got: {other:?}"),
                }
                match obj.get("count") {
                    Some(Document::Number(aws_smithy_types::Number::PosInt(n))) => assert_eq!(*n, 3),
                    other => panic!("expected count=Number::PosInt, got: {other:?}"),
                }
                match obj.get("enabled") {
                    Some(Document::Bool(b)) => assert!(*b),
                    other => panic!("expected enabled=Bool, got: {other:?}"),
                }
                match obj.get("nested") {
                    Some(Document::Object(inner)) => match inner.get("inner") {
                        Some(Document::String(s)) => assert_eq!(s, "deep"),
                        other => panic!("expected nested.inner=String, got: {other:?}"),
                    },
                    other => panic!("expected nested=Object, got: {other:?}"),
                }

                // Annotation custom trait is stored as AnnotationTrait
                let ann_id = aws_smithy_schema::shape_id!("test", "myAnnotationCustomTrait");
                assert!(traits.get(&ann_id).is_some(), "should include annotation custom trait");

                assert_eq!(traits.len(), 3);
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `nested aggregate recursive through a struct keeps its resolved element schema and xmlName`() {
        // Wrapper -> values: OuterList -> (element) InnerMap -> value: Wrapper. The
        // cycle passes through the struct Wrapper, which carries its own ::SCHEMA
        // constant, so it is not an aggregate cycle: the serializer references
        // InnerMap's resolved nested schema constant (preserving @xmlName on the map
        // value) instead of substituting prelude::DOCUMENT. The model has no document
        // shapes, so any prelude::DOCUMENT in the generated schema would mean the
        // element's member traits were dropped.
        val nestedModel =
            """
            namespace test
            structure Wrapper { values: OuterList }
            list OuterList { member: InnerMap }
            map InnerMap {
                key: String,
                @xmlName("CustomValue")
                value: Wrapper
            }
            """.asSmithyModel()
        val nestedContext = testCodegenContext(nestedModel)
        val writer = RustWriter.forModule("model")
        SchemaGenerator(nestedContext, writer, nestedModel.lookup<StructureShape>("test#Wrapper")).render()
        val rendered = writer.toString()

        // The InnerMap element keeps its resolved schema; no prelude::DOCUMENT
        // substitution for a model with no document shapes.
        rendered shouldNotContain "prelude::DOCUMENT"
        // The resolved nested schema carries the map value's @xmlName.
        rendered shouldContain "with_xml_name(\"CustomValue\")"
    }

    @Test
    fun `client cbor serialization omits target metadata through members and collections`() {
        val targetModel =
            """
            namespace test
            @error("client")
            structure Failure { message: String }
            union Choice { failure: Failure, unit: Unit }
            list Failures { member: Failure }
            map FailureMap { key: String, value: Failure }
            structure Envelope {
                failure: Failure,
                choice: Choice,
                list: Failures,
                map: FailureMap
            }
            """.asSmithyModel()
        val targetProvider = testSymbolProvider(targetModel)
        val targetContext = testCodegenContext(targetModel)
        val project = TestWorkspace.testProject(targetProvider)
        val filter =
            SchemaTraitFilter(targetModel, setOf(software.amazon.smithy.model.shapes.ShapeId.from("smithy.api#error")))
        for (name in listOf("Failure", "Envelope")) {
            val target = targetModel.lookup<StructureShape>("test#$name")
            project.useShapeWriter(target) {
                renderStructWithSchema(this, targetModel, targetProvider, targetContext, target, project, filter)
            }
        }
        val choice = targetModel.lookup<UnionShape>("test#Choice")
        project.useShapeWriter(choice) {
            UnionGenerator(targetModel, targetProvider, this, choice).render()
            SchemaGenerator(targetContext, this, choice, filter).render()
            rustTemplate(
                "use #{CborCodec};",
                "CborCodec" to RuntimeType.smithyCbor(targetContext.runtimeConfig).resolve("codec::CborCodec"),
            )
            unitTest(
                "client_nested_values_report_shape_identity",
                """
                use aws_smithy_schema::{shape_id, Schema, ShapeType};
                use aws_smithy_schema::codec::Codec;
                use aws_smithy_schema::serde::ShapeSerializer;
                use crate::test_error::Failure;
                use aws_smithy_schema::codec::FinishSerializer;
                let failure = || Failure { message: Some("failed".into()) };
                let envelope = Envelope {
                    failure: Some(failure()),
                    choice: Some(Choice::Failure(failure())),
                    list: Some(vec![failure()]),
                    map: Some(std::collections::HashMap::from([("key".into(), failure())])),
                };
                let codec = CborCodec::default();
                let mut serializer = codec.create_serializer().with_struct_prefix(|schema, _| {
                    assert!(schema.member_name().is_none());
                    assert!(["Envelope", "Choice", "Failure"].contains(&schema.shape_id().shape_name()));
                    Ok(())
                });
                serializer.write_struct(Envelope::SCHEMA, &envelope).unwrap();
                let bytes = serializer.finish();
                fn error(encoder: &mut aws_smithy_cbor::Encoder) {
                    encoder.begin_map().str("message").str("failed").end();
                }
                let mut expected = aws_smithy_cbor::Encoder::new(Vec::new());
                expected.begin_map().str("failure");
                error(&mut expected);
                expected.str("choice").begin_map().str("failure");
                error(&mut expected);
                expected.end().str("list").begin_array();
                error(&mut expected);
                expected.end().str("map").begin_map().str("key");
                error(&mut expected);
                expected.end().end();
                assert_eq!(bytes, expected.into_writer());
                let result = Envelope::deserialize(&mut codec.create_deserializer(&bytes)).unwrap();
                assert_eq!(result.failure.unwrap().message.as_deref(), Some("failed"));
                // Unit values report the prelude shape, while the member still names the variant.
                let mut serializer = codec.create_serializer().with_struct_prefix(|target, _| {
                    assert!(target.member_name().is_none());
                    assert!(::std::ptr::eq(target, Choice::SCHEMA) || ::std::ptr::eq(target, &aws_smithy_schema::prelude::UNIT));
                    Ok(())
                });
                serializer.write_struct(Choice::SCHEMA, &Choice::Unit).unwrap();
                assert_eq!(serializer.finish(), vec![0xbf, 0x64, b'u', b'n', b'i', b't', 0xbf, 0xff, 0xff]);
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `schema for recursive structure compiles`() {
        val recursiveModel =
            RecursiveShapeBoxer().transform(
                """
                namespace test
                structure TreeNode {
                    value: String,
                    children: TreeNodeList
                }
                list TreeNodeList {
                    member: TreeNode
                }
                structure LinkedNode {
                    value: String,
                    next: LinkedNode
                }
                """.asSmithyModel(),
            )

        val recProvider = testSymbolProvider(recursiveModel)
        val recContext = testCodegenContext(recursiveModel)
        val project = TestWorkspace.testProject(recProvider)

        // Recursive through a list
        val treeNode = recursiveModel.lookup<StructureShape>("test#TreeNode")
        project.useShapeWriter(treeNode) {
            renderStructWithSchema(this, recursiveModel, recProvider, recContext, treeNode, project)
            unitTest(
                "recursive_via_list",
                """
                use aws_smithy_schema::{Schema, ShapeType};
                let schema = TreeNode::SCHEMA;
                assert_eq!(schema.shape_type(), ShapeType::Structure);
                assert_eq!(schema.member_schema("children").unwrap().shape_type(), ShapeType::List);
                assert!(format!("{schema:?}").len() < 20_000);
                """,
            )
        }

        // Directly recursive (uses Box via RecursiveShapeBoxer)
        val linkedNode = recursiveModel.lookup<StructureShape>("test#LinkedNode")
        project.useShapeWriter(linkedNode) {
            renderStructWithSchema(this, recursiveModel, recProvider, recContext, linkedNode, project)
            unitTest(
                "directly_recursive",
                """
                use aws_smithy_schema::{Schema, ShapeType};
                let schema = LinkedNode::SCHEMA;
                assert_eq!(schema.shape_type(), ShapeType::Structure);
                assert_eq!(schema.member_schema("next").unwrap().shape_type(), ShapeType::Structure);
                assert!(format!("{schema:?}").len() < 20_000);
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `SchemaStructureCustomization auto-generates schema with StructureGenerator`() {
        val project = TestWorkspace.testProject(provider)
        val shape = model.lookup<StructureShape>("test#MyStruct")
        project.useShapeWriter(shape) {
            // Schema is generated automatically via the customization — no separate SchemaGenerator call
            StructureGenerator(
                model,
                provider,
                this,
                shape,
                listOf(SchemaStructureCustomization(codegenContext)),
                StructSettings(flattenVecAccessors = true),
            ).render()
            this.implBlock(provider.toSymbol(shape)) {
                BuilderGenerator.renderConvenienceMethod(this, provider, shape)
            }
            project.withModule(provider.moduleForBuilder(shape)) {
                BuilderGenerator(model, provider, shape, emptyList()).render(this)
            }
            unitTest(
                "auto_schema",
                """
                use aws_smithy_schema::{Schema, ShapeType};
                let schema = MyStruct::SCHEMA;
                assert_eq!(schema.shape_type(), ShapeType::Structure);
                assert_eq!(schema.shape_id().as_str(), "test#MyStruct");
                assert!(schema.member_schema("name").is_some());
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `json round trip with ComplexStruct`() {
        val project = TestWorkspace.testProject(provider)
        val myStruct = model.lookup<StructureShape>("test#MyStruct")
        val complexStruct = model.lookup<StructureShape>("test#ComplexStruct")
        project.useShapeWriter(myStruct) {
            renderStructWithSchema(this, model, provider, codegenContext, myStruct, project)
        }
        project.useShapeWriter(complexStruct) {
            renderStructWithSchema(this, model, provider, codegenContext, complexStruct, project)
            // Pull in JsonCodec dependency
            rustTemplate(
                "use #{JsonCodec};",
                "JsonCodec" to RuntimeType.smithyJson(codegenContext.runtimeConfig).resolve("codec::JsonCodec"),
            )
            unitTest(
                "json_round_trip_complex_struct",
                """
                use aws_smithy_schema::serde::{SerializableStruct, ShapeSerializer};
                use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
                use aws_smithy_schema::codec::Codec;
                use aws_smithy_types::{Blob, DateTime};
                use std::collections::HashMap;

                // Build a ComplexStruct with all fields populated.
                let mut metadata = HashMap::new();
                metadata.insert("env".to_string(), "prod".to_string());
                metadata.insert("region".to_string(), "us-west-2".to_string());

                let original = ComplexStruct {
                    label: Some("test-label".to_string()),
                    count: Some(42),
                    ratio: Some(3.15),
                    enabled: Some(true),
                    data: Some(Blob::new(vec![1, 2, 3, 4, 5])),
                    created_at: Some(DateTime::from_secs(1700000000)),
                    nested: Some(MyStruct {
                        name: Some("Alice".to_string()),
                        age: Some(30),
                        active: Some(true),
                    }),
                    tags: Some(vec!["alpha".to_string(), "beta".to_string()]),
                    metadata: Some(metadata),
                };

                // Serialize
                let codec = JsonCodec::new(JsonCodecSettings::default());
                let mut ser = codec.create_serializer();
                ser.write_struct(ComplexStruct::SCHEMA, &original).expect("serialization should succeed");
                let bytes = ser.finish();

                // Deserialize
                let mut deser = codec.create_deserializer(&bytes);
                let result = ComplexStruct::deserialize(&mut deser).expect("deserialization should succeed");

                // Assert all fields round-tripped
                assert_eq!(result.label, Some("test-label".to_string()));
                assert_eq!(result.count, Some(42));
                assert_eq!(result.ratio, Some(3.15));
                assert_eq!(result.enabled, Some(true));
                assert_eq!(result.data, Some(Blob::new(vec![1, 2, 3, 4, 5])));
                assert_eq!(result.created_at, Some(DateTime::from_secs(1700000000)));

                // Nested struct
                let nested = result.nested.expect("nested should be Some");
                assert_eq!(nested.name, Some("Alice".to_string()));
                assert_eq!(nested.age, Some(30));
                assert_eq!(nested.active, Some(true));

                // List
                assert_eq!(result.tags, Some(vec!["alpha".to_string(), "beta".to_string()]));

                // Map (compare entries individually since HashMap order is non-deterministic)
                let meta = result.metadata.expect("metadata should be Some");
                assert_eq!(meta.len(), 2);
                assert_eq!(meta.get("env"), Some(&"prod".to_string()));
                assert_eq!(meta.get("region"), Some(&"us-west-2".to_string()));
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `json round trip with nested aggregates`() {
        val project = TestWorkspace.testProject(provider)
        val myStruct = model.lookup<StructureShape>("test#MyStruct")
        val myUnion = model.lookup<UnionShape>("test#MyUnion")
        val nestedAgg = model.lookup<StructureShape>("test#NestedAggregates")
        project.useShapeWriter(myStruct) {
            renderStructWithSchema(this, model, provider, codegenContext, myStruct, project)
        }
        project.useShapeWriter(myUnion) {
            UnionGenerator(model, provider, this, myUnion).render()
            SchemaGenerator(codegenContext, this, myUnion).render()
        }
        project.useShapeWriter(nestedAgg) {
            renderStructWithSchema(this, model, provider, codegenContext, nestedAgg, project)
            rustTemplate(
                "use #{JsonCodec};",
                "JsonCodec" to RuntimeType.smithyJson(codegenContext.runtimeConfig).resolve("codec::JsonCodec"),
            )
            unitTest(
                "nested_aggregates_round_trip",
                """
                use aws_smithy_schema::serde::{SerializableStruct, ShapeSerializer};
                use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
                use aws_smithy_schema::codec::Codec;
                use std::collections::HashMap;

                let schema = NestedAggregates::SCHEMA;

                // Build a NestedAggregates with all fields populated.
                let mut struct_map = HashMap::new();
                struct_map.insert("alice".to_string(), MyStruct {
                    name: Some("Alice".to_string()), age: Some(30), active: Some(true),
                });

                let union_list = vec![
                    MyUnion::StringVariant("hello".to_string()),
                    MyUnion::IntVariant(42),
                ];

                let mut inner_map = HashMap::new();
                inner_map.insert("k1".to_string(), "v1".to_string());
                let mut map_of_maps = HashMap::new();
                map_of_maps.insert("outer".to_string(), inner_map);

                let original = NestedAggregates {
                    struct_map: Some(struct_map),
                    union_list: Some(union_list),
                    map_of_maps: Some(map_of_maps),
                };

                // Serialize
                let codec = JsonCodec::new(JsonCodecSettings::default());
                let mut ser = codec.create_serializer();
                ser.write_struct(NestedAggregates::SCHEMA, &original).expect("serialization");
                let bytes = ser.finish();

                // Deserialize
                let mut deser = codec.create_deserializer(&bytes);
                let result = NestedAggregates::deserialize(&mut deser).expect("deserialization");

                // Assert struct map
                let sm = result.struct_map.expect("struct_map");
                let alice = sm.get("alice").expect("alice");
                assert_eq!(alice.name, Some("Alice".to_string()));
                assert_eq!(alice.age, Some(30));

                // Assert union list
                let ul = result.union_list.expect("union_list");
                assert_eq!(ul.len(), 2);
                assert!(matches!(&ul[0], MyUnion::StringVariant(s) if s == "hello"));
                assert!(matches!(&ul[1], MyUnion::IntVariant(42)));

                // Assert map of maps
                let mm = result.map_of_maps.expect("map_of_maps");
                let inner = mm.get("outer").expect("outer");
                assert_eq!(inner.get("k1"), Some(&"v1".to_string()));
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `json round trip with nested sparse aggregates`() {
        val project = TestWorkspace.testProject(provider)
        val shape = model.lookup<StructureShape>("test#SparseNestedAggregates")
        project.useShapeWriter(shape) {
            renderStructWithSchema(this, model, provider, codegenContext, shape, project)
            rustTemplate(
                "use #{JsonCodec};",
                "JsonCodec" to RuntimeType.smithyJson(codegenContext.runtimeConfig).resolve("codec::JsonCodec"),
            )
            unitTest(
                "nested_sparse_aggregates_round_trip",
                """
                use aws_smithy_schema::serde::{SerializableStruct, ShapeSerializer};
                use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
                use aws_smithy_schema::codec::Codec;
                use std::collections::HashMap;

                // `@sparse` applies to the collection that carries it, at any nesting depth, so
                // every inner collection below generates as a collection of `Option`s even when the
                // collection containing it is dense. This is the shape that broke `iotsitewise`:
                // `list RowList { member: Result }` where `Result` is an `@sparse list<String>`.
                let mut map_of_sparse_lists = HashMap::new();
                map_of_sparse_lists.insert("row".to_string(), vec![Some("v".to_string()), None]);

                let mut sparse_map = HashMap::new();
                sparse_map.insert("present".to_string(), Some("yes".to_string()));
                sparse_map.insert("absent".to_string(), None);

                let mut map_of_sparse_maps = HashMap::new();
                map_of_sparse_maps.insert("outer".to_string(), sparse_map.clone());

                let original = SparseNestedAggregates {
                    // dense list of sparse lists
                    list_of_sparse_lists: Some(vec![vec![Some("a".to_string()), None]]),
                    // map whose value is a sparse list
                    map_of_sparse_lists: Some(map_of_sparse_lists),
                    // sparse list of sparse lists
                    sparse_list_of_sparse_lists: Some(vec![Some(vec![None, Some("b".to_string())]), None]),
                    // dense list of sparse maps
                    list_of_sparse_maps: Some(vec![sparse_map]),
                    // map whose value is a sparse map
                    map_of_sparse_maps: Some(map_of_sparse_maps),
                };

                let codec = JsonCodec::new(JsonCodecSettings::default());
                let mut ser = codec.create_serializer();
                ser.write_struct(SparseNestedAggregates::SCHEMA, &original).expect("serialization");
                let bytes = ser.finish();

                let mut deser = codec.create_deserializer(&bytes);
                let result = SparseNestedAggregates::deserialize(&mut deser).expect("deserialization");

                // Nulls inside the nested sparse collections must survive as `None`.
                let lol = result.list_of_sparse_lists.expect("list_of_sparse_lists");
                assert_eq!(lol, vec![vec![Some("a".to_string()), None]]);

                let mol = result.map_of_sparse_lists.expect("map_of_sparse_lists");
                assert_eq!(mol.get("row"), Some(&vec![Some("v".to_string()), None]));

                let slol = result.sparse_list_of_sparse_lists.expect("sparse_list_of_sparse_lists");
                assert_eq!(slol, vec![Some(vec![None, Some("b".to_string())]), None]);

                let lom = result.list_of_sparse_maps.expect("list_of_sparse_maps");
                assert_eq!(lom[0].get("present"), Some(&Some("yes".to_string())));
                assert_eq!(lom[0].get("absent"), Some(&None));

                let mom = result.map_of_sparse_maps.expect("map_of_sparse_maps");
                assert_eq!(mom.get("outer").expect("outer").get("absent"), Some(&None));
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `collection helper methods used for simple list and map deserialization`() {
        val project = TestWorkspace.testProject(provider)
        val shape = model.lookup<StructureShape>("test#CollectionHelperStruct")
        project.useShapeWriter(shape) {
            renderStructWithSchema(this, model, provider, codegenContext, shape, project)
            rustTemplate(
                "use #{JsonCodec};",
                "JsonCodec" to RuntimeType.smithyJson(codegenContext.runtimeConfig).resolve("codec::JsonCodec"),
            )
            unitTest(
                "collection_helpers_round_trip",
                """
                use aws_smithy_schema::serde::{SerializableStruct, ShapeSerializer};
                use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
                use aws_smithy_schema::codec::Codec;
                use aws_smithy_types::Blob;
                use std::collections::HashMap;

                let mut string_map = HashMap::new();
                string_map.insert("k1".to_string(), "v1".to_string());
                string_map.insert("k2".to_string(), "v2".to_string());

                let original = CollectionHelperStruct {
                    string_list: Some(vec!["a".to_string(), "b".to_string(), "c".to_string()]),
                    blob_list: Some(vec![Blob::new(vec![1, 2]), Blob::new(vec![3, 4])]),
                    int_list: Some(vec![10, 20, 30]),
                    long_list: Some(vec![100, 200, 300]),
                    string_string_map: Some(string_map),
                };

                let codec = JsonCodec::new(JsonCodecSettings::default());
                let mut ser = codec.create_serializer();
                ser.write_struct(CollectionHelperStruct::SCHEMA, &original).expect("serialize");
                let bytes = ser.finish();

                let mut deser = codec.create_deserializer(&bytes);
                let result = CollectionHelperStruct::deserialize(&mut deser).expect("deserialize");

                assert_eq!(result.string_list, Some(vec!["a".to_string(), "b".to_string(), "c".to_string()]));
                assert_eq!(result.blob_list, Some(vec![Blob::new(vec![1, 2]), Blob::new(vec![3, 4])]));
                assert_eq!(result.int_list, Some(vec![10, 20, 30]));
                assert_eq!(result.long_list, Some(vec![100, 200, 300]));
                let meta = result.string_string_map.expect("map");
                assert_eq!(meta.len(), 2);
                assert_eq!(meta.get("k1"), Some(&"v1".to_string()));
                assert_eq!(meta.get("k2"), Some(&"v2".to_string()));
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `response header bindings are parsed by the selected protocol`() {
        val headerModel =
            """
            namespace test

            structure HeaderBound {
                @httpHeader("x-str")
                scalar: String

                @httpHeader("x-int")
                number: Integer

                @httpHeader("x-int-list")
                intList: IntList

                @httpHeader("x-str-list")
                stringList: StringList

                @httpHeader("x-date")
                date: Timestamp

                @httpHeader("x-encoded")
                encoded: EncodedString

                @httpPrefixHeaders("X-Meta-")
                metadata: StringMap
            }

            list IntList { member: Integer }
            list StringList { member: String }
            map StringMap { key: String, value: String }

            @mediaType("application/json")
            string EncodedString
            """.asSmithyModel()
        val headerProvider = testSymbolProvider(headerModel)
        val headerContext = testCodegenContext(headerModel)
        val project = TestWorkspace.testProject(headerProvider)
        val shape = headerModel.lookup<StructureShape>("test#HeaderBound")
        project.useShapeWriter(shape) {
            renderStructWithSchema(this, headerModel, headerProvider, headerContext, shape, project)
            val generated = toString()
            // The shape parses nothing itself and has no HTTP-shaped methods: the selected
            // protocol's response deserializer reads the bindings from the schema. These
            // assertions are the source-level half of the behavior the Rust test below exercises.
            generated shouldNotContain "deserialize_with_response"
            generated shouldNotContain "get_all_bytes"
            generated shouldNotContain "read_many_primitive_bytes"
            generated shouldNotContain "headers_for_prefix"
            generated shouldNotContain "NonUtf8HeaderHandling"
            // The header names themselves do remain, on the member schemas. That is the point of
            // the design: the names are data the runtime reads, not literals in parsing code.

            rustTemplate(
                """
                ##[allow(unused_imports)]
                use #{JsonCodec} as _;
                ##[allow(unused_imports)]
                use #{HeaderMap} as _;
                ##[allow(unused_imports)]
                use #{Headers} as _;
                """,
                "JsonCodec" to RuntimeType.smithyJson(headerContext.runtimeConfig).resolve("codec::JsonCodec"),
                "HeaderMap" to CargoDependency.Http1x.toType().resolve("HeaderMap"),
                "Headers" to
                    CargoDependency.smithyRuntimeApi(headerContext.runtimeConfig)
                        .withFeature("http-1x")
                        .toType()
                        .resolve("http::Headers"),
            )
            unitTest(
                "schema_http_header_bindings",
                """
                use aws_smithy_schema::protocol::ClientProtocolInner;
                use aws_smithy_schema::serde::SerdeError;
                use aws_smithy_runtime_api::http::{Headers, NonUtf8HeaderHandling, Response, StatusCode};
                use aws_smithy_types::body::SdkBody;
                use aws_smithy_types::config_bag::ConfigBag;

                fn headers(entries: &[(&'static str, &'static [u8])]) -> Headers {
                    let mut raw = http_1x::HeaderMap::new();
                    for (name, value) in entries {
                        raw.append(
                            *name,
                            http_1x::HeaderValue::from_bytes(value).unwrap(),
                        );
                    }
                    Headers::try_from(raw).unwrap()
                }

                // Reads the response the way a generated client does: through the selected
                // protocol, here restJson1, which owns the HTTP bindings.
                fn read(headers: &Headers, cfg: &ConfigBag) -> Result<HeaderBound, SerdeError> {
                    let mut response = Response::new(
                        StatusCode::try_from(200u16).unwrap(),
                        SdkBody::from("{}"),
                    );
                    *response.headers_mut() = headers.clone();
                    let protocol = aws_smithy_json::protocol::aws_rest_json_1::AwsRestJsonProtocol::new();
                    let mut deser = protocol.deserialize_response(&response, HeaderBound::SCHEMA, cfg)?;
                    HeaderBound::deserialize(&mut *deser)
                }

                let default_cfg = ConfigBag::base();
                let mut reject_cfg = ConfigBag::base();
                reject_cfg
                    .interceptor_state()
                    .store_put(NonUtf8HeaderHandling::Reject);
                let mut skip_cfg = ConfigBag::base();
                skip_cfg
                    .interceptor_state()
                    .store_put(NonUtf8HeaderHandling::Skip);

                // Repeated lines, quoted list values, commas in scalar strings and HTTP dates,
                // media-type decoding, and case-normalized prefix matching all use the same
                // parser as the legacy generated path.
                let happy = headers(&[
                    ("x-str", b"value,with,commas"),
                    ("x-int", b"42"),
                    ("x-int-list", b"1, 2"),
                    ("x-int-list", b"3"),
                    ("x-str-list", br#""a,b", "quote\"d", plain"#),
                    ("x-date", b"Sun, 06 Nov 1994 08:49:37 GMT"),
                    ("x-encoded", b"aGVsbG8="),
                    ("x-meta-one", b"first"),
                    ("x-meta-two", b"second"),
                ]);
                let out = read(&happy, &default_cfg).unwrap();
                assert_eq!(out.scalar.as_deref(), Some("value,with,commas"));
                assert_eq!(out.number, Some(42));
                assert_eq!(out.int_list, Some(vec![1, 2, 3]));
                assert_eq!(
                    out.string_list,
                    Some(vec!["a,b".to_string(), "quote\"d".to_string(), "plain".to_string()]),
                );
                assert!(out.date.is_some());
                assert_eq!(out.encoded.as_deref(), Some("hello"));
                let metadata = out.metadata.unwrap();
                assert_eq!(metadata.get("one").map(String::as_str), Some("first"));
                assert_eq!(metadata.get("two").map(String::as_str), Some("second"));

                let unreadable_scalar = headers(&[("x-str", b"value-\xe9")]);
                let err = read(&unreadable_scalar, &default_cfg)
                    .expect_err("an empty ConfigBag also defaults to Reject");
                let message = err.to_string();
                assert!(message.contains("scalar"), "{message}");
                assert!(message.contains("x-str"), "{message}");

                let err = read(&unreadable_scalar, &reject_cfg)
                    .expect_err("an explicit Reject policy must reject");
                let message = err.to_string();
                assert!(message.contains("scalar"), "{message}");
                assert!(message.contains("x-str"), "{message}");

                let skipped = read(&unreadable_scalar, &skip_cfg).unwrap();
                assert_eq!(skipped.scalar, None);
                assert_eq!(unreadable_scalar.get_bytes("x-str"), Some(&b"value-\xe9"[..]));

                // Skip applies only when the bound member has an unreadable raw value. Valid UTF-8
                // with a malformed value remains an error.
                for malformed in [
                    headers(&[("x-int", b"not-an-integer")]),
                    headers(&[("x-date", b"not-a-date")]),
                    headers(&[("x-encoded", b"not-base64!")]),
                    // Valid base64 whose decoded payload is not UTF-8 is also a real parse error.
                    headers(&[("x-encoded", b"6Q==")]),
                ] {
                    read(&malformed, &skip_cfg)
                        .expect_err("Skip must not hide readable malformed values");
                }

                // A scalar receiving repeated lines is a cardinality error, not last-write-wins.
                let repeated_scalar = headers(&[("x-int", b"1"), ("x-int", b"2")]);
                read(&repeated_scalar, &skip_cfg)
                    .expect_err("repeated scalar values must fail cardinality checks");

                // If one list value is unreadable, Skip applies to the whole member regardless of
                // whether a separately malformed readable value appears before or after it.
                for mixed in [
                    headers(&[
                        ("x-int-list", b"value-\xe9"),
                        ("x-int-list", b"not-an-integer"),
                    ]),
                    headers(&[
                        ("x-int-list", b"not-an-integer"),
                        ("x-int-list", b"value-\xe9"),
                    ]),
                ] {
                    read(&mixed, &default_cfg).expect_err("Reject reports either parse failure");
                    let skipped = read(&mixed, &skip_cfg).unwrap();
                    assert_eq!(skipped.int_list, None);
                    assert_eq!(
                        mixed
                            .get_all_bytes("x-int-list")
                            .filter(|value| std::str::from_utf8(value).is_err())
                            .collect::<Vec<_>>(),
                        vec![&b"value-\xe9"[..]],
                    );
                }

                // No matching prefix headers produces Some(empty), preserving existing Smithy
                // response binding behavior.
                let empty = read(&headers(&[]), &default_cfg).unwrap();
                assert_eq!(empty.metadata, Some(Default::default()));

                // One unreadable prefix entry skips the entire map rather than returning only its
                // readable entries. Raw values remain available to interceptors.
                let unreadable_prefix = headers(&[
                    ("x-meta-good", b"readable"),
                    ("x-meta-bad", b"value-\xe9"),
                ]);
                let err = read(&unreadable_prefix, &default_cfg)
                    .expect_err("Reject must fail an unreadable prefix entry");
                let message = err.to_string();
                assert!(message.contains("metadata"), "{message}");
                assert!(message.contains("X-Meta-"), "{message}");
                assert_eq!(read(&unreadable_prefix, &skip_cfg).unwrap().metadata, None);
                assert_eq!(unreadable_prefix.get_bytes("x-meta-good"), Some(&b"readable"[..]));
                assert_eq!(unreadable_prefix.get_bytes("x-meta-bad"), Some(&b"value-\xe9"[..]));

                // Repeated lines for a prefixed scalar value follow the same cardinality rule as a
                // normal scalar header rather than silently overwriting the map entry.
                let repeated_prefix = headers(&[
                    ("x-meta-duplicate", b"first"),
                    ("x-meta-duplicate", b"second"),
                ]);
                read(&repeated_prefix, &skip_cfg)
                    .expect_err("repeated prefix values must fail cardinality checks");
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `null values in JSON are skipped during struct deserialization`() {
        val project = TestWorkspace.testProject(provider)
        val shape = model.lookup<StructureShape>("test#MyStruct")
        project.useShapeWriter(shape) {
            renderStructWithSchema(this, model, provider, codegenContext, shape, project)
            rustTemplate(
                "use #{JsonCodec};",
                "JsonCodec" to RuntimeType.smithyJson(codegenContext.runtimeConfig).resolve("codec::JsonCodec"),
            )
            unitTest(
                "null_values_skipped",
                """
                use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
                use aws_smithy_schema::codec::Codec;

                // JSON with some fields set to null and some present
                let json = br#"{"name":"hello","age":null,"active":true}"#;
                let codec = JsonCodec::new(JsonCodecSettings::default());
                let mut deser = codec.create_deserializer(json);
                let result = MyStruct::deserialize(&mut deser).expect("deserialize");

                assert_eq!(result.name, Some("hello".to_string()));
                assert_eq!(result.age, None);
                assert_eq!(result.active, Some(true));
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `target-level streaming is propagated into combined member schemas`() {
        // `@streaming`'s selector is `:is(blob, union)`, so it is applied to the target
        // shape and a member can never carry it directly. Without propagation a combined
        // member schema cannot tell a streaming payload — whose live body or event
        // receiver is owned by the streaming call site — from a buffered one the protocol
        // should read itself.
        val streamingModel =
            """
            namespace test

            @streaming
            blob StreamingBlob

            blob BufferedBlob

            structure StreamingOutput {
                @httpPayload
                streamingBody: StreamingBlob,

                bufferedBody: BufferedBlob,

                name: String
            }
            """.asSmithyModel()
        val ctx = testCodegenContext(streamingModel)
        val writer = RustWriter.forModule("model")
        SchemaGenerator(ctx, writer, streamingModel.lookup<StructureShape>("test#StreamingOutput")).render()
        val rendered = writer.toString()

        // Exactly one member gains the trait: the one whose target is streaming.
        rendered shouldContain "with_streaming()"
        val streamingSetters = Regex("with_streaming\\(\\)").findAll(rendered).count()
        assert(streamingSetters == 1) {
            "expected exactly 1 `.with_streaming()` (the streaming member), found $streamingSetters in:\n$rendered"
        }

        // The streaming member keeps its own `@httpPayload` too, so the payload branch can
        // both recognize the member and know it is streaming.
        rendered shouldContain "with_http_payload()"
    }

    @Test
    fun `a streaming member is consumed without overwriting the installed stream`() {
        // A streaming member's value belongs to the operation's response path, which installs the
        // live stream or event receiver on the builder. The member consumer must therefore assign
        // nothing — but it must still advance a cursor-based codec past the value, or every member
        // after it desynchronizes.
        //
        // Both halves used to be wrong. A streaming union emitted `todo!("deserialize streaming
        // union")`, which panics, and a streaming blob emitted `ByteStream::new(SdkBody::empty())`,
        // which silently replaced a real stream with an empty one. Neither is reachable through a
        // protocol that owns HTTP bindings, because it routes a streaming `@httpPayload` member away
        // from the codec, but `deserialize` is also called directly — by a body-only protocol and by
        // the type registry — with whatever the body happens to contain.
        val streamingModel =
            """
            namespace test

            structure Event { data: String }

            @streaming
            union EventStream { event: Event }

            @streaming
            blob StreamingBlob

            structure EventStreamMixed {
                @httpPayload
                events: EventStream,

                name: String
            }

            structure StreamingBlobMixed {
                @httpPayload
                body: StreamingBlob,

                name: String
            }
            """.asSmithyModel()
        val streamingProvider = testSymbolProvider(streamingModel)
        val streamingContext = testCodegenContext(streamingModel)

        // The union half is asserted at the source level only. Compiling it would mean generating the
        // union and its event structure too, and the arm is produced by the same branch either way —
        // the decision is made from the member's target, not from which streaming kind it is. The
        // end-to-end evidence for the union is that a regenerated event-stream client no longer
        // contains `todo!("deserialize streaming union")` anywhere.
        val unionWriter = RustWriter.forModule("model")
        SchemaGenerator(streamingContext, unionWriter, streamingModel.lookup<StructureShape>("test#EventStreamMixed"))
            .render()
        val unionRendered = unionWriter.toString()
        unionRendered shouldContain "deser.skip_value()?;"
        unionRendered shouldNotContain "deserialize streaming union"

        val project = TestWorkspace.testProject(streamingProvider)
        val blobShape = streamingModel.lookup<StructureShape>("test#StreamingBlobMixed")
        project.useShapeWriter(blobShape) {
            renderStructWithSchema(this, streamingModel, streamingProvider, streamingContext, blobShape, project)
            val generated = toString()
            // The arm skips rather than assigning. Pinning the absence of the old expression is what
            // keeps a future edit from reintroducing the silent replacement.
            generated shouldContain "deser.skip_value()?;"
            generated shouldNotContain "SdkBody::empty()"
        }

        project.lib {
            rustTemplate(
                "##[allow(unused_imports)] use #{JsonCodec} as _;",
                "JsonCodec" to RuntimeType.smithyJson(streamingContext.runtimeConfig).resolve("codec::JsonCodec"),
            )
            unitTest(
                "a_streaming_member_is_skipped_not_replaced",
                """
                use aws_smithy_schema::codec::Codec;
                let codec = aws_smithy_json::codec::JsonCodec::new(Default::default());
                // `body` precedes `name`, so if the streaming member were declined without advancing
                // the cursor the `name` read would fail or read the wrong value.
                let body = br##"{"body":"aGVsbG8=","name":"kept"}"##;
                let mut deser = codec.create_deserializer(body);
                let out = crate::test_model::StreamingBlobMixed::deserialize(&mut deser)
                    .expect("a streaming member in the body must not fail the parse");
                // The body carried "hello" for the streaming member. Nothing was read into it, so it
                // holds only whatever finalization defaults to. Asserting emptiness rather than
                // `None` is what this symbol provider allows: it maps a streaming blob to `Blob`
                // rather than to `ByteStream`, so the field is not optional here.
                assert!(
                    out.body.as_ref().is_empty(),
                    "the body value must not be read into a streaming member, got {:?}",
                    out.body,
                );
                assert_eq!(Some("kept"), out.name.as_deref(), "members after the streaming one must still parse");
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `target-level streaming is propagated for an event stream union member`() {
        // The union half of `@streaming`'s selector: an event-stream member must be
        // distinguishable the same way a streaming blob is.
        val eventStreamModel =
            """
            namespace test

            structure Event { data: String }

            @streaming
            union EventStream { event: Event }

            structure EventStreamOutput {
                @httpPayload
                events: EventStream,

                name: String
            }
            """.asSmithyModel()
        val ctx = testCodegenContext(eventStreamModel)
        val writer = RustWriter.forModule("model")
        SchemaGenerator(
            ctx,
            writer,
            eventStreamModel.lookup<StructureShape>("test#EventStreamOutput"),
        ).render()
        val rendered = writer.toString()

        val streamingSetters = Regex("with_streaming\\(\\)").findAll(rendered).count()
        assert(streamingSetters == 1) {
            "expected exactly 1 `.with_streaming()` (the event stream member), found " +
                "$streamingSetters in:\n$rendered"
        }
    }

    @Test
    fun `a structure with no streaming targets emits no streaming setter`() {
        // Guards against propagating the trait to every member.
        val plainModel =
            """
            namespace test
            structure PlainOutput {
                body: Blob,
                name: String
            }
            """.asSmithyModel()
        val ctx = testCodegenContext(plainModel)
        val writer = RustWriter.forModule("model")
        SchemaGenerator(ctx, writer, plainModel.lookup<StructureShape>("test#PlainOutput")).render()
        writer.toString() shouldNotContain "with_streaming()"
    }
}
