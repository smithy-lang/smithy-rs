/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
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
}
