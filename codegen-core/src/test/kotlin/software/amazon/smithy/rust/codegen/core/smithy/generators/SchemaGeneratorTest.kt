/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
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
            intVariant: Integer
        }
        """.asSmithyModel()

    private val provider = testSymbolProvider(model)
    private val codegenContext = testCodegenContext(model)

    @Test
    fun `schema for structure compiles and works at runtime`() {
        val project = TestWorkspace.testProject(provider)
        val shape = model.lookup<StructureShape>("test#MyStruct")
        project.useShapeWriter(shape) {
            StructureGenerator(
                model,
                provider,
                this,
                shape,
                emptyList(),
                StructSettings(flattenVecAccessors = true),
            ).render()
            SchemaGenerator(codegenContext, this, shape).render()
            unitTest(
                "schema_structure",
                """
                use aws_smithy_schema::Schema;
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
            StructureGenerator(
                model,
                provider,
                this,
                shape,
                emptyList(),
                StructSettings(flattenVecAccessors = true),
            ).render()
            SchemaGenerator(codegenContext, this, shape).render()
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
            StructureGenerator(
                model,
                provider,
                this,
                myStruct,
                emptyList(),
                StructSettings(flattenVecAccessors = true),
            ).render()
            SchemaGenerator(codegenContext, this, myStruct).render()
        }
        project.useShapeWriter(complexStruct) {
            StructureGenerator(
                model,
                provider,
                this,
                complexStruct,
                emptyList(),
                StructSettings(flattenVecAccessors = true),
            ).render()
            SchemaGenerator(codegenContext, this, complexStruct).render()
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
                let schema = MyUnion::SCHEMA;
                assert_eq!(schema.shape_type(), aws_smithy_schema::ShapeType::Union);
                assert!(schema.member_schema("StringVariant").is_some());
                assert!(schema.member_schema("IntVariant").is_some());
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
            StructureGenerator(
                model,
                provider,
                this,
                shape,
                emptyList(),
                StructSettings(flattenVecAccessors = true),
            ).render()
            SchemaGenerator(codegenContext, this, shape).render()
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
            StructureGenerator(
                model,
                provider,
                this,
                shape,
                emptyList(),
                StructSettings(flattenVecAccessors = true),
            ).render()
            SchemaGenerator(codegenContext, this, shape).render()
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
            StructureGenerator(traitModel, traitProvider, this, shape, emptyList(), StructSettings(flattenVecAccessors = true)).render()
            SchemaGenerator(traitContext, this, shape).render()
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

            @myCustomTrait(setting: "hello")
            @myAnnotationCustomTrait
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
                ),
            )
        val project = TestWorkspace.testProject(customProvider)
        val shape = customTraitModel.lookup<StructureShape>("test#Tagged")
        project.useShapeWriter(shape) {
            StructureGenerator(customTraitModel, customProvider, this, shape, emptyList(), StructSettings(flattenVecAccessors = true)).render()
            SchemaGenerator(customContext, this, shape, filter).render()
            unitTest(
                "unknown_traits",
                """
                use aws_smithy_schema::{DocumentTrait, Trait};
                let s = Tagged::SCHEMA;

                // Unknown traits are stored in the fallback TraitMap
                let traits = s.traits().expect("should have a fallback trait map");

                // Complex custom trait is stored as DocumentTrait
                let custom_id = aws_smithy_schema::shape_id!("test", "myCustomTrait");
                let custom = traits.get(&custom_id).expect("should include custom trait");
                let doc_trait = custom.as_any().downcast_ref::<DocumentTrait>()
                    .expect("unknown complex trait should be a DocumentTrait");
                match doc_trait.value() {
                    aws_smithy_types::Document::String(json) => {
                        assert!(json.contains("hello"), "should contain the setting value: {json}");
                    }
                    other => panic!("expected Document::String, got: {other:?}"),
                }

                // Annotation custom trait is stored as AnnotationTrait
                let ann_id = aws_smithy_schema::shape_id!("test", "myAnnotationCustomTrait");
                assert!(traits.get(&ann_id).is_some(), "should include annotation custom trait");

                assert_eq!(traits.len(), 2);
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
            StructureGenerator(
                recursiveModel,
                recProvider,
                this,
                treeNode,
                emptyList(),
                StructSettings(flattenVecAccessors = true),
            ).render()
            SchemaGenerator(recContext, this, treeNode).render()
            unitTest(
                "recursive_via_list",
                """
                use aws_smithy_schema::{Schema, ShapeType};
                let schema = TreeNode::SCHEMA;
                assert_eq!(schema.shape_type(), ShapeType::Structure);
                assert_eq!(schema.member_schema("children").unwrap().shape_type(), ShapeType::List);
                """,
            )
        }

        // Directly recursive (uses Box via RecursiveShapeBoxer)
        val linkedNode = recursiveModel.lookup<StructureShape>("test#LinkedNode")
        project.useShapeWriter(linkedNode) {
            StructureGenerator(
                recursiveModel,
                recProvider,
                this,
                linkedNode,
                emptyList(),
                StructSettings(flattenVecAccessors = true),
            ).render()
            SchemaGenerator(recContext, this, linkedNode).render()
            unitTest(
                "directly_recursive",
                """
                use aws_smithy_schema::{Schema, ShapeType};
                let schema = LinkedNode::SCHEMA;
                assert_eq!(schema.shape_type(), ShapeType::Structure);
                assert_eq!(schema.member_schema("next").unwrap().shape_type(), ShapeType::Structure);
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
            StructureGenerator(
                model,
                provider,
                this,
                myStruct,
                emptyList(),
                StructSettings(flattenVecAccessors = true),
            ).render()
            SchemaGenerator(codegenContext, this, myStruct).render()
        }
        project.useShapeWriter(complexStruct) {
            StructureGenerator(
                model,
                provider,
                this,
                complexStruct,
                emptyList(),
                StructSettings(flattenVecAccessors = true),
            ).render()
            SchemaGenerator(codegenContext, this, complexStruct).render()
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
}
