/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import io.kotest.matchers.shouldBe
import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.REDACTION
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.core.util.orNull

class EnumGeneratorTest {
    @Nested
    inner class EnumMemberModelTests {
        private val testModel = """
            namespace test
            @enum([
                { value: "some-value-1",
                  name: "some_name_1",
                  documentation: "Some documentation." },
                { value: "some-value-2",
                  name: "someName2",
                  documentation: "More documentation #escape" },
                { value: "unknown",
                  name: "unknown",
                  documentation: "It has some docs that #need to be escaped" }
            ])
            string EnumWithUnknown
        """.asSmithyModel()
        private val symbolProvider = testSymbolProvider(testModel)

        private val enumTrait = testModel.lookup<StringShape>("test#EnumWithUnknown").expectTrait<EnumTrait>()

        private fun model(name: String): EnumMemberModel =
            EnumMemberModel(enumTrait.values.first { it.name.orNull() == name }, symbolProvider)

        @Test
        fun `it converts enum names to PascalCase and renames any named Unknown to UnknownValue`() {
            model("some_name_1").derivedName() shouldBe "SomeName1"
            model("someName2").also { someName2 ->
                someName2.derivedName() shouldBe "SomeName2"
                someName2.name()!!.renamedFrom shouldBe null
            }
            model("unknown").also { unknown ->
                unknown.derivedName() shouldBe "UnknownValue"
                unknown.name()!!.renamedFrom shouldBe "Unknown"
            }
        }

        @Test
        fun `it should render documentation`() {
            val rendered = RustWriter.forModule("model").also { model("some_name_1").render(it) }.toString()
            rendered shouldContain
                """
                /// Some documentation.
                SomeName1,
                """.trimIndent()
        }

        @Test
        fun `it adds a documentation note when renaming an enum named Unknown`() {
            val rendered = RustWriter.forModule("model").also { model("unknown").render(it) }.toString()
            println(rendered.lines())
            rendered shouldContain
                """
                /// It has some docs that #need to be escaped
                ///
                /// _Note: `::Unknown` has been renamed to `::UnknownValue`._
                UnknownValue,
                """.trimIndent()
        }
    }

    @Nested
    inner class EnumGeneratorTests {
        @Test
        fun `it generates named enums`() {
            val model = """
                namespace test
                @enum([
                    {
                        value: "t2.nano",
                        name: "T2_NANO",
                        documentation: "T2 instances are Burstable Performance Instances.",
                        tags: ["ebsOnly"]
                    },
                    {
                        value: "t2.micro",
                        name: "T2_MICRO",
                        documentation: "T2 instances are Burstable Performance Instances.",
                        deprecated: true,
                        tags: ["ebsOnly"]
                    },
                ])
                @deprecated(since: "1.2.3")
                string InstanceType
            """.asSmithyModel()

            val shape = model.lookup<StringShape>("test#InstanceType")
            val trait = shape.expectTrait<EnumTrait>()
            val provider = testSymbolProvider(model)
            val project = TestWorkspace.testProject(provider)
            project.withModule(RustModule.Model) {
                rust("##![allow(deprecated)]")
                val generator = EnumGenerator(model, provider, this, shape, trait)
                generator.render()
                unitTest(
                    "it_generates_named_enums",
                    """
                    let instance = InstanceType::T2Micro;
                    assert_eq!(instance.as_str(), "t2.micro");
                    assert_eq!(InstanceType::from("t2.nano"), InstanceType::T2Nano);
                    assert_eq!(InstanceType::from("other"), InstanceType::Unknown(crate::types::UnknownVariantValue("other".to_owned())));
                    // round trip unknown variants:
                    assert_eq!(InstanceType::from("other").as_str(), "other");
                    """,
                )
                val output = toString()
                output.shouldContain("#[non_exhaustive]")
                // on enum variant `T2Micro`
                output.shouldContain("#[deprecated]")
                // on enum itself
                output.shouldContain("#[deprecated(since = \"1.2.3\")]")
            }
            project.compileAndTest()
        }

        @Test
        fun `named enums implement eq and hash`() {
            val model = """
                namespace test
                @enum([
                {
                    value: "Foo",
                    name: "Foo",
                },
                {
                    value: "Bar",
                    name: "Bar"
                }])
                string FooEnum
            """.asSmithyModel()

            val shape = model.lookup<StringShape>("test#FooEnum")
            val trait = shape.expectTrait<EnumTrait>()
            val provider = testSymbolProvider(model)
            val project = TestWorkspace.testProject(provider)
            project.withModule(RustModule.Model) {
                val generator = EnumGenerator(model, provider, this, shape, trait)
                generator.render()
                unitTest(
                    "named_enums_implement_eq_and_hash",
                    """
                    assert_eq!(FooEnum::Foo, FooEnum::Foo);
                    assert_ne!(FooEnum::Bar, FooEnum::Foo);
                    let mut hash_of_enums = std::collections::HashSet::new();
                    hash_of_enums.insert(FooEnum::Foo);
                    """.trimIndent(),
                )
            }
            project.compileAndTest()
        }

        @Test
        fun `unnamed enums implement eq and hash`() {
            val model = """
                namespace test
                @enum([
                {
                    value: "Foo",
                },
                {
                    value: "Bar",
                }])
                @deprecated
                string FooEnum
            """.asSmithyModel()

            val shape = model.lookup<StringShape>("test#FooEnum")
            val trait = shape.expectTrait<EnumTrait>()
            val provider = testSymbolProvider(model)
            val project = TestWorkspace.testProject(provider)
            project.withModule(RustModule.Model) {
                rust("##![allow(deprecated)]")
                val generator = EnumGenerator(model, provider, this, shape, trait)
                generator.render()
                unitTest(
                    "unnamed_enums_implement_eq_and_hash",
                    """
                    assert_eq!(FooEnum::from("Foo"), FooEnum::from("Foo"));
                    assert_ne!(FooEnum::from("Bar"), FooEnum::from("Foo"));
                    let mut hash_of_enums = std::collections::HashSet::new();
                    hash_of_enums.insert(FooEnum::from("Foo"));
                    """.trimIndent(),
                )
            }
            project.compileAndTest()
        }

        @Test
        fun `it generates unnamed enums`() {
            val model = """
                namespace test
                @enum([
                    {
                        value: "Foo",
                    },
                    {
                        value: "Baz",
                    },
                    {
                        value: "Bar",
                    },
                    {
                        value: "1",
                    },
                    {
                        value: "0",
                    },
                ])
                string FooEnum
            """.asSmithyModel()

            val shape = model.lookup<StringShape>("test#FooEnum")
            val trait = shape.expectTrait<EnumTrait>()
            val provider = testSymbolProvider(model)
            val project = TestWorkspace.testProject(provider)
            project.withModule(RustModule.Model) {
                rust("##![allow(deprecated)]")
                val generator = EnumGenerator(model, provider, this, shape, trait)
                generator.render()
                unitTest(
                    "it_generates_unnamed_enums",
                    """
                    // Values should be sorted
                    assert_eq!(FooEnum::${EnumGenerator.Values}(), ["0", "1", "Bar", "Baz", "Foo"]);
                    """.trimIndent(),
                )
            }
            project.compileAndTest()
        }

        @Test
        fun `it escapes the Unknown variant if the enum has an unknown value in the model`() {
            val model = """
                namespace test
                @enum([
                    { name: "Known", value: "Known" },
                    { name: "Unknown", value: "Unknown" },
                    { name: "UnknownValue", value: "UnknownValue" },
                ])
                string SomeEnum
            """.asSmithyModel()

            val shape = model.lookup<StringShape>("test#SomeEnum")
            val trait = shape.expectTrait<EnumTrait>()
            val provider = testSymbolProvider(model)
            val project = TestWorkspace.testProject(provider)
            project.withModule(RustModule.Model) {
                val generator = EnumGenerator(model, provider, this, shape, trait)
                generator.render()
                unitTest(
                    "it_escapes_the_unknown_variant_if_the_enum_has_an_unknown_value_in_the_model",
                    """
                    assert_eq!(SomeEnum::from("Unknown"), SomeEnum::UnknownValue);
                    assert_eq!(SomeEnum::from("UnknownValue"), SomeEnum::UnknownValue_);
                    assert_eq!(SomeEnum::from("SomethingNew"), SomeEnum::Unknown(crate::types::UnknownVariantValue("SomethingNew".to_owned())));
                    """.trimIndent(),
                )
            }
            project.compileAndTest()
        }

        @Test
        fun `it should generate documentation for enums`() {
            val model = """
                namespace test

                /// Some top-level documentation.
                @enum([
                    { name: "Known", value: "Known" },
                    { name: "Unknown", value: "Unknown" },
                ])
                string SomeEnum
            """.asSmithyModel()

            val shape = model.lookup<StringShape>("test#SomeEnum")
            val trait = shape.expectTrait<EnumTrait>()
            val provider = testSymbolProvider(model)
            val project = TestWorkspace.testProject(provider)
            project.withModule(RustModule.Model) {
                val generator = EnumGenerator(model, provider, this, shape, trait)
                generator.render()
                val rendered = toString()
                rendered shouldContain
                    """
                    /// Some top-level documentation.
                    ///
                    /// _Note: `SomeEnum::Unknown` has been renamed to `::UnknownValue`._
                    """.trimIndent()
            }
            project.compileAndTest()
        }

        @Test
        fun `it should generate documentation for unnamed enums`() {
            val model = """
                namespace test

                /// Some top-level documentation.
                @enum([
                    { value: "One" },
                    { value: "Two" },
                ])
                string SomeEnum
            """.asSmithyModel()

            val shape = model.lookup<StringShape>("test#SomeEnum")
            val trait = shape.expectTrait<EnumTrait>()
            val provider = testSymbolProvider(model)
            val project = TestWorkspace.testProject(provider)
            project.withModule(RustModule.Model) {
                val generator = EnumGenerator(model, provider, this, shape, trait)
                generator.render()
                val rendered = toString()
                rendered shouldContain
                    """
                    /// Some top-level documentation.
                    """.trimIndent()
            }
            project.compileAndTest()
        }
    }

    @Test
    fun `it handles variants that clash with Rust reserved words`() {
        val model = """
            namespace test
            @enum([
                { name: "Known", value: "Known" },
                { name: "Self", value: "other" },
            ])
            string SomeEnum
        """.asSmithyModel()

        val shape = model.lookup<StringShape>("test#SomeEnum")
        val trait = shape.expectTrait<EnumTrait>()
        val provider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(provider)
        project.withModule(RustModule.Model) {
            val generator = EnumGenerator(model, provider, this, shape, trait)
            generator.render()
            unitTest(
                "it_handles_variants_that_clash_with_rust_reserved_words",
                """
                assert_eq!(SomeEnum::from("other"), SomeEnum::SelfValue);
                assert_eq!(SomeEnum::from("SomethingNew"), SomeEnum::Unknown(crate::types::UnknownVariantValue("SomethingNew".to_owned())));
                """.trimIndent(),
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `matching on enum should be forward-compatible`() {
        fun expectMatchExpressionCompiles(model: Model, shapeId: String, enumToMatchOn: String) {
            val shape = model.lookup<StringShape>(shapeId)
            val trait = shape.expectTrait<EnumTrait>()
            val provider = testSymbolProvider(model)
            val project = TestWorkspace.testProject(provider)
            project.withModule(RustModule.Model) {
                val generator = EnumGenerator(model, provider, this, shape, trait)
                generator.render()
                unitTest(
                    "matching_on_enum_should_be_forward_compatible",
                    """
                    match $enumToMatchOn {
                        SomeEnum::Variant1 => assert!(false, "expected `Variant3` but got `Variant1`"),
                        SomeEnum::Variant2 => assert!(false, "expected `Variant3` but got `Variant2`"),
                        other @ _ if other.as_str() == "Variant3" => assert!(true),
                        _ => assert!(false, "expected `Variant3` but got `_`"),
                    }
                    """.trimIndent(),
                )
            }
            project.compileAndTest()
        }

        val modelV1 = """
            namespace test

            @enum([
                { name: "Variant1", value: "Variant1" },
                { name: "Variant2", value: "Variant2" },
            ])
            string SomeEnum
        """.asSmithyModel()
        val variant3AsUnknown = """SomeEnum::from("Variant3")"""
        expectMatchExpressionCompiles(modelV1, "test#SomeEnum", variant3AsUnknown)

        val modelV2 = """
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
        val model = """
            namespace test
            @enum([
                { name: "Foo", value: "Foo" },
                { name: "Bar", value: "Bar" },
            ])
            string SomeEnum
        """.asSmithyModel()

        val shape = model.lookup<StringShape>("test#SomeEnum")
        val trait = shape.expectTrait<EnumTrait>()
        val provider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(provider)
        project.withModule(RustModule.Model) {
            val generator = EnumGenerator(model, provider, this, shape, trait)
            generator.render()
            unitTest(
                "impl_debug_for_non_sensitive_enum_should_implement_the_derived_debug_trait",
                """
                assert_eq!(format!("{:?}", SomeEnum::Foo), "Foo");
                assert_eq!(format!("{:?}", SomeEnum::Bar), "Bar");
                assert_eq!(
                    format!("{:?}", SomeEnum::from("Baz")),
                    "Unknown(UnknownVariantValue(\"Baz\"))"
                );
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `impl debug for sensitive enum should redact text`() {
        val model = """
            namespace test
            @sensitive
            @enum([
                { name: "Foo", value: "Foo" },
                { name: "Bar", value: "Bar" },
            ])
            string SomeEnum
        """.asSmithyModel()

        val shape = model.lookup<StringShape>("test#SomeEnum")
        val trait = shape.expectTrait<EnumTrait>()
        val provider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(provider)
        project.withModule(RustModule.Model) {
            val generator = EnumGenerator(model, provider, this, shape, trait)
            generator.render()
            unitTest(
                "impl_debug_for_sensitive_enum_should_redact_text",
                """
                assert_eq!(format!("{:?}", SomeEnum::Foo), $REDACTION);
                assert_eq!(format!("{:?}", SomeEnum::Bar), $REDACTION);
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `impl debug for non-sensitive unnamed enum should implement the derived debug trait`() {
        val model = """
            namespace test
            @enum([
                { value: "Foo" },
                { value: "Bar" },
            ])
            string SomeEnum
        """.asSmithyModel()

        val shape = model.lookup<StringShape>("test#SomeEnum")
        val trait = shape.expectTrait<EnumTrait>()
        val provider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(provider)
        project.withModule(RustModule.Model) {
            val generator = EnumGenerator(model, provider, this, shape, trait)
            generator.render()
            unitTest(
                "impl_debug_for_non_sensitive_unnamed_enum_should_implement_the_derived_debug_trait",
                """
                for variant in SomeEnum::values() {
                    assert_eq!(
                        format!("{:?}", SomeEnum(variant.to_string())),
                        format!("SomeEnum(\"{}\")", variant.to_owned())
                    );
                }
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `impl debug for sensitive unnamed enum should redact text`() {
        val model = """
            namespace test
            @sensitive
            @enum([
                { value: "Foo" },
                { value: "Bar" },
            ])
            string SomeEnum
        """.asSmithyModel()

        val shape = model.lookup<StringShape>("test#SomeEnum")
        val trait = shape.expectTrait<EnumTrait>()
        val provider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(provider)
        project.withModule(RustModule.Model) {
            val generator = EnumGenerator(model, provider, this, shape, trait)
            generator.render()
            unitTest(
                "impl_debug_for_sensitive_unnamed_enum_should_redact_text",
                """
                for variant in SomeEnum::values() {
                    assert_eq!(
                        format!("{:?}", SomeEnum(variant.to_string())),
                        $REDACTION
                    );
                }
                """,
            )
        }
        project.compileAndTest()
    }
}
