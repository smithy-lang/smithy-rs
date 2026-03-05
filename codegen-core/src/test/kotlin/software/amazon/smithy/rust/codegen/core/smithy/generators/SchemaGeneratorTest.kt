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
import software.amazon.smithy.rust.codegen.core.util.dq
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
            StructureGenerator(model, provider, this, shape, emptyList(), StructSettings(flattenVecAccessors = true)).render()
            SchemaGenerator(codegenContext, this, shape).render()
            unitTest(
                "schema_structure",
                """
                use aws_smithy_schema::Schema;
                // Use a reference to access Schema methods
                let s = MyStruct { name: None, age: None, active: None };
                assert_eq!(s.shape_type(), aws_smithy_schema::ShapeType::Structure);
                assert_eq!(s.shape_id().as_str(), "test#MyStruct");
                // member lookup by name
                assert!(s.member_schema("name").is_some());
                assert!(s.member_schema("age").is_some());
                assert!(s.member_schema("active").is_some());
                assert!(s.member_schema("nonexistent").is_none());
                // member lookup by index
                let (name, _) = s.member_schema_by_index(0).expect("index 0");
                assert_eq!(name, "name");
                // members iterator
                let names: Vec<&str> = s.members().map(|(n, _)| n).collect();
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
            StructureGenerator(model, provider, this, shape, emptyList(), StructSettings(flattenVecAccessors = true)).render()
            SchemaGenerator(codegenContext, this, shape).render()
            unitTest(
                "member_schema_types",
                """
                use aws_smithy_schema::{Schema, ShapeType};
                let s = MyStruct { name: None, age: None, active: None };
                let name_schema = s.member_schema("name").unwrap();
                assert_eq!(name_schema.shape_type(), ShapeType::String);
                assert_eq!(name_schema.member_name(), Some("name"));
                assert_eq!(name_schema.member_index(), Some(0));

                let age_schema = s.member_schema("age").unwrap();
                assert_eq!(age_schema.shape_type(), ShapeType::Integer);
                assert_eq!(age_schema.member_index(), Some(1));

                let active_schema = s.member_schema("active").unwrap();
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
            StructureGenerator(model, provider, this, myStruct, emptyList(), StructSettings(flattenVecAccessors = true)).render()
            SchemaGenerator(codegenContext, this, myStruct).render()
        }
        project.useShapeWriter(complexStruct) {
            StructureGenerator(model, provider, this, complexStruct, emptyList(), StructSettings(flattenVecAccessors = true)).render()
            SchemaGenerator(codegenContext, this, complexStruct).render()
            unitTest(
                "complex_schema",
                """
                use aws_smithy_schema::{Schema, ShapeType};
                let s = ComplexStruct {
                    label: None, count: None, ratio: None, enabled: None,
                    data: None, created_at: None, nested: None, tags: None, metadata: None,
                };
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

                // All 9 members present via iterator
                let names: Vec<&str> = s.members().map(|(n, _)| n).collect();
                assert_eq!(names.len(), 9);

                // Index-based access consistent with iterator order
                for (i, (name, _)) in s.members().enumerate() {
                    let (idx_name, _) = s.member_schema_by_index(i).unwrap();
                    assert_eq!(name, idx_name);
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
                let u = MyUnion::StringVariant("hello".into());
                assert_eq!(u.shape_type(), aws_smithy_schema::ShapeType::Union);
                assert!(u.member_schema("StringVariant").is_some());
                assert!(u.member_schema("IntVariant").is_some());
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
                use aws_smithy_schema::{Schema, ShapeId, Trait};
                use aws_smithy_schema::traits::SensitiveTrait;
                let s = SecretData { name: None, password: None, old_field: None };

                // @sensitive is included and uses the typed SensitiveTrait
                let sensitive_id = ShapeId::new("smithy.api#sensitive");
                assert!(s.traits().contains(&sensitive_id), "should include @sensitive");
                let sensitive = s.traits().get(&sensitive_id).unwrap();
                assert!(sensitive.as_any().downcast_ref::<SensitiveTrait>().is_some(),
                    "should be a typed SensitiveTrait");

                // @deprecated is NOT in the inclusion list
                let deprecated_id = ShapeId::new("smithy.api#deprecated");
                assert!(!s.traits().contains(&deprecated_id), "should exclude @deprecated");

                assert_eq!(s.traits().len(), 1);
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `unknown traits stored as DocumentTrait`() {
        val customTraitModel =
            """
            namespace test

            @trait(selector: "structure")
            structure myCustomTrait {
                setting: String
            }

            @trait(selector: "structure")
            @tags(["custom"])
            structure myAnnotationCustomTrait {}

            @myCustomTrait(setting: "hello")
            @myAnnotationCustomTrait
            structure Tagged {
                value: String
            }
            """.asSmithyModel()

        val customProvider = testSymbolProvider(customTraitModel)
        val customContext = testCodegenContext(customTraitModel)
        // Add the custom traits to the filter so they're included
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
                use aws_smithy_schema::{Schema, ShapeId, Trait, DocumentTrait};
                let s = Tagged { value: None };

                // Complex custom trait is stored as DocumentTrait
                let custom_id = ShapeId::new("test#myCustomTrait");
                assert!(s.traits().contains(&custom_id), "should include custom trait");
                let custom = s.traits().get(&custom_id).unwrap();
                let doc_trait = custom.as_any().downcast_ref::<DocumentTrait>()
                    .expect("unknown complex trait should be a DocumentTrait");
                // The value is stored as a JSON string
                match doc_trait.value() {
                    aws_smithy_types::Document::String(json) => {
                        assert!(json.contains("hello"), "should contain the setting value: {json}");
                    }
                    other => panic!("expected Document::String, got: {other:?}"),
                }

                // Annotation custom trait is stored as AnnotationTrait
                let ann_id = ShapeId::new("test#myAnnotationCustomTrait");
                assert!(s.traits().contains(&ann_id), "should include annotation custom trait");

                assert_eq!(s.traits().len(), 2);
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `custom TraitCodegenProvider overrides rendering`() {
        val customTraitModel =
            """
            namespace test

            @trait(selector: "structure")
            structure myCustomTrait {
                setting: String
            }

            @myCustomTrait(setting: "world")
            structure Overridden {
                value: String
            }
            """.asSmithyModel()

        val customProvider = testSymbolProvider(customTraitModel)
        val customContext = testCodegenContext(customTraitModel)
        val filter =
            SchemaTraitFilter(
                customTraitModel,
                setOf(software.amazon.smithy.model.shapes.ShapeId.from("test#myCustomTrait")),
            )
        val extension = SchemaTraitExtension()
        extension.add(software.amazon.smithy.model.shapes.ShapeId.from("test#myCustomTrait")) { trait ->
            // Render as a StringTrait with the "setting" value extracted
            val node = trait.toNode().expectObjectNode()
            val setting = node.getStringMember("setting").get().value
            software.amazon.smithy.rust.codegen.core.rustlang.writable {
                rustTemplate(
                    """Box::new(#{StringTrait}::new(#{ShapeId}::new("test##myCustomTrait"), ${setting.dq()}))""",
                    "StringTrait" to RuntimeType.smithySchema(customContext.runtimeConfig).resolve("StringTrait"),
                    "ShapeId" to RuntimeType.smithySchema(customContext.runtimeConfig).resolve("ShapeId"),
                )
            }
        }
        val project = TestWorkspace.testProject(customProvider)
        val shape = customTraitModel.lookup<StructureShape>("test#Overridden")
        project.useShapeWriter(shape) {
            StructureGenerator(customTraitModel, customProvider, this, shape, emptyList(), StructSettings(flattenVecAccessors = true)).render()
            SchemaGenerator(customContext, this, shape, filter, extension).render()
            unitTest(
                "custom_provider",
                """
                use aws_smithy_schema::{Schema, ShapeId, Trait, StringTrait};
                let s = Overridden { value: None };
                let id = ShapeId::new("test#myCustomTrait");
                let t = s.traits().get(&id).unwrap();
                let st = t.as_any().downcast_ref::<StringTrait>()
                    .expect("custom provider should render as StringTrait");
                assert_eq!(st.value(), "world");
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
            StructureGenerator(recursiveModel, recProvider, this, treeNode, emptyList(), StructSettings(flattenVecAccessors = true)).render()
            SchemaGenerator(recContext, this, treeNode).render()
            unitTest(
                "recursive_via_list",
                """
                use aws_smithy_schema::{Schema, ShapeType};
                let node = TreeNode { value: None, children: None };
                assert_eq!(node.shape_type(), ShapeType::Structure);
                assert_eq!(node.member_schema("children").unwrap().shape_type(), ShapeType::List);
                """,
            )
        }

        // Directly recursive (uses Box via RecursiveShapeBoxer)
        val linkedNode = recursiveModel.lookup<StructureShape>("test#LinkedNode")
        project.useShapeWriter(linkedNode) {
            StructureGenerator(recursiveModel, recProvider, this, linkedNode, emptyList(), StructSettings(flattenVecAccessors = true)).render()
            SchemaGenerator(recContext, this, linkedNode).render()
            unitTest(
                "directly_recursive",
                """
                use aws_smithy_schema::{Schema, ShapeType};
                let node = LinkedNode { value: None, next: None };
                assert_eq!(node.shape_type(), ShapeType::Structure);
                assert_eq!(node.member_schema("next").unwrap().shape_type(), ShapeType::Structure);
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
                let s = MyStruct { name: None, age: None, active: None };
                assert_eq!(s.shape_type(), ShapeType::Structure);
                assert_eq!(s.shape_id().as_str(), "test#MyStruct");
                assert!(s.member_schema("name").is_some());
                """,
            )
        }
        project.compileAndTest()
    }
}
