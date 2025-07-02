/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.rust.codegen.client.testutil.testClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup

class ClientEnumGeneratorTest {
    @Test
    fun `matching on enum should be forward-compatible`() {
        fun expectMatchExpressionCompiles(
            model: Model,
            shapeId: String,
            enumToMatchOn: String,
        ) {
            val shape = model.lookup<StringShape>(shapeId)
            val context = testClientCodegenContext(model)
            val project = TestWorkspace.testProject(context.symbolProvider)
            project.moduleFor(shape) {
                ClientEnumGenerator(context, shape, emptyList()).render(this)
                unitTest(
                    "matching_on_enum_should_be_forward_compatible",
                    """
                    match $enumToMatchOn {
                        SomeEnum::Variant1 => panic!("expected `Variant3` but got `Variant1`"),
                        SomeEnum::Variant2 => panic!("expected `Variant3` but got `Variant2`"),
                        other @ _ if other.as_str() == "Variant3" => {},
                        _ => panic!("expected `Variant3` but got `_`"),
                    }
                    """,
                )
            }
            project.compileAndTest()
        }

        val modelV1 =
            """
            namespace test

            @enum([
                { name: "Variant1", value: "Variant1" },
                { name: "Variant2", value: "Variant2" },
            ])
            string SomeEnum
            """.asSmithyModel()
        val variant3AsUnknown = """SomeEnum::from("Variant3")"""
        expectMatchExpressionCompiles(modelV1, "test#SomeEnum", variant3AsUnknown)

        val modelV2 =
            """
            namespace test

            @enum([
                { name: "Variant1", value: "Variant1" },
                { name: "Variant2", value: "Variant2" },
                { name: "Variant3", value: "Variant3" },
            ])
            string SomeEnum
            """.asSmithyModel()
        val variant3AsVariant3 = "SomeEnum::Variant3"
        expectMatchExpressionCompiles(modelV2, "test#SomeEnum", variant3AsVariant3)
    }

    @Test
    fun `impl debug for non-sensitive enum should implement the derived debug trait`() {
        val model =
            """
            namespace test
            @enum([
                { name: "Foo", value: "Foo" },
                { name: "Bar", value: "Bar" },
            ])
            string SomeEnum
            """.asSmithyModel()

        val shape = model.lookup<StringShape>("test#SomeEnum")
        val context = testClientCodegenContext(model)
        val project = TestWorkspace.testProject(context.symbolProvider)
        project.moduleFor(shape) {
            ClientEnumGenerator(context, shape, emptyList()).render(this)
            unitTest(
                "impl_debug_for_non_sensitive_enum_should_implement_the_derived_debug_trait",
                """
                assert_eq!(format!("{:?}", SomeEnum::Foo), "Foo");
                assert_eq!(format!("{:?}", SomeEnum::Bar), "Bar");
                assert_eq!(format!("{}", SomeEnum::Foo), "Foo");
                assert_eq!(SomeEnum::Bar.to_string(), "Bar");
                assert_eq!(
                    format!("{:?}", SomeEnum::from("Baz")),
                    "Unknown(UnknownVariantValue(\"Baz\"))"
                );
                """,
            )
        }
        project.compileAndTest()
    }

    // The idempotency of RustModules paired with Smithy's alphabetic order shape iteration meant that
    // if an @sensitive enum was the first enum generated that the opaque type underlying the Unknown
    // variant would not derive Debug, breaking all non-@sensitive enums
    @Test
    fun `sensitive enum in earlier alphabetic order does not break non-sensitive enums`() {
        val model =
            """
            namespace test

            @sensitive
            @enum([
                { name: "Foo", value: "Foo" },
                { name: "Bar", value: "Bar" },
            ])
            string FooA

            @enum([
                { name: "Baz", value: "Baz" },
                { name: "Ahh", value: "Ahh" },
            ])
            string FooB
            """.asSmithyModel()

        val shapeA = model.lookup<StringShape>("test#FooA")
        val shapeB = model.lookup<StringShape>("test#FooB")
        val context = testClientCodegenContext(model)
        val project = TestWorkspace.testProject(context.symbolProvider)
        project.moduleFor(shapeA) {
            ClientEnumGenerator(context, shapeA).render(this)
        }
        project.moduleFor(shapeB) {
            ClientEnumGenerator(context, shapeB).render(this)
            unitTest(
                "impl_debug_for_non_sensitive_enum_should_implement_the_derived_debug_trait",
                """
                assert_eq!(format!("{:?}", FooB::Baz), "Baz");
                assert_eq!(format!("{:?}", FooB::Ahh), "Ahh");
                assert_eq!(format!("{}", FooB::Baz), "Baz");
                assert_eq!(FooB::Ahh.to_string(), "Ahh");
                assert_eq!(
                    format!("{:?}", FooB::from("Bar")),
                    "Unknown(UnknownVariantValue(\"Bar\"))"
                );
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `it escapes the Unknown variant if the enum has an unknown value in the model`() {
        val model =
            """
            namespace test
            @enum([
                { name: "Known", value: "Known" },
                { name: "Unknown", value: "Unknown" },
                { name: "UnknownValue", value: "UnknownValue" },
            ])
            string SomeEnum
            """.asSmithyModel()

        val shape = model.lookup<StringShape>("test#SomeEnum")
        val context = testClientCodegenContext(model)
        val project = TestWorkspace.testProject(context.symbolProvider)
        project.moduleFor(shape) {
            ClientEnumGenerator(context, shape, emptyList()).render(this)
            unitTest(
                "it_escapes_the_unknown_variant_if_the_enum_has_an_unknown_value_in_the_model",
                """
                assert_eq!(SomeEnum::from("Unknown"), SomeEnum::UnknownValue);
                assert_eq!(SomeEnum::from("UnknownValue"), SomeEnum::UnknownValue_);
                assert_eq!(
                    SomeEnum::from("SomethingNew"),
                    SomeEnum::Unknown(crate::primitives::sealed_enum_unknown::UnknownVariantValue("SomethingNew".to_owned()))
                );
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `generated named enums can roundtrip between string and enum value on the unknown variant`() {
        val model =
            """
            namespace test
            @enum([
                { value: "t2.nano", name: "T2_NANO" },
                { value: "t2.micro", name: "T2_MICRO" },
            ])
            string InstanceType
            """.asSmithyModel()

        val shape = model.lookup<StringShape>("test#InstanceType")
        val context = testClientCodegenContext(model)
        val project = TestWorkspace.testProject(context.symbolProvider)
        project.moduleFor(shape) {
            rust("##![allow(deprecated)]")
            ClientEnumGenerator(context, shape, emptyList()).render(this)
            unitTest(
                "generated_named_enums_roundtrip",
                """
                let instance = InstanceType::T2Micro;
                assert_eq!(instance.as_str(), "t2.micro");
                assert_eq!(InstanceType::from("t2.nano"), InstanceType::T2Nano);
                assert_eq!(instance.to_string(), "t2.micro");
                // round trip unknown variants:
                assert_eq!(
                    InstanceType::from("other"),
                    InstanceType::Unknown(crate::primitives::sealed_enum_unknown::UnknownVariantValue("other".to_owned()))
                );
                assert_eq!(InstanceType::from("other").as_str(), "other");
                assert_eq!(InstanceType::from("other").to_string(), "other");
                """,
            )
        }
        project.compileAndTest()
    }
}
