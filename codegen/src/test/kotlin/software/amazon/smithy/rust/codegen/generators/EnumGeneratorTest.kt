/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.generators

import io.kotest.matchers.shouldBe
import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.smithy.generators.EnumGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.EnumMemberModel
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.util.expectTrait
import software.amazon.smithy.rust.codegen.util.lookup
import software.amazon.smithy.rust.codegen.util.orNull

class EnumGeneratorTest {
    @Nested
    inner class EnumMemberModelTests {
        private val testModel = """
            namespace test
            enum EnumWithUnknown {
                  @documentation("Some documentation.")
                  some_name_1 = "some-value-1",

                  @documentation("More documentation #escape")
                  someName2 = "some-value-2",

                  @documentation("It has some docs that #need to be escaped")
                  unknown = "unknown",
            }
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
                @deprecated(since: "1.2.3")
                enum InstanceType {
                    @documentation("T2 instances are Burstable Performance Instances.")
                    @tags(["ebsOnly"])
                    T2_NANO = "t2.nano",
        
                    @documentation("T2 instances are Burstable Performance Instances.")
                    @tags(["ebsOnly"])
                    @deprecated
                    T2_MICRO = "t2.micro",
                }
            """.asSmithyModel()
            val provider = testSymbolProvider(model)
            val writer = RustWriter.forModule("model")
            writer.rust("##![allow(deprecated)]")
            val shape = model.lookup<StringShape>("test#InstanceType")
            val generator = EnumGenerator(model, provider, writer, shape, shape.expectTrait<EnumTrait>())
            generator.render()
            writer.compileAndTest(
                """
                let instance = InstanceType::T2Micro;
                assert_eq!(instance.as_str(), "t2.micro");
                assert_eq!(InstanceType::from("t2.nano"), InstanceType::T2Nano);
                assert_eq!(InstanceType::from("other"), InstanceType::Unknown("other".to_owned()));
                // round trip unknown variants:
                assert_eq!(InstanceType::from("other").as_str(), "other");
                """,
            )
            val output = writer.toString()
            output shouldContain "#[non_exhaustive]"
            // on enum variant `T2Micro`
            output shouldContain "#[deprecated]"
            // on enum itself
            output shouldContain "#[deprecated(since = \"1.2.3\")]"
        }

        @Test
        fun `named enums implement eq and hash`() {
            val model = """
                namespace test
                enum FooEnum {
                    Foo = "Foo",
                    Bar = "Bar",
                }
            """.asSmithyModel()
            val shape = model.lookup<StringShape>("test#FooEnum")
            val trait = shape.expectTrait<EnumTrait>()
            val writer = RustWriter.forModule("model")
            val generator = EnumGenerator(model, testSymbolProvider(model), writer, shape, trait)
            generator.render()
            writer.compileAndTest(
                """
                assert_eq!(FooEnum::Foo, FooEnum::Foo);
                assert_ne!(FooEnum::Bar, FooEnum::Foo);
                let mut hash_of_enums = std::collections::HashSet::new();
                hash_of_enums.insert(FooEnum::Foo);
                """.trimIndent(),
            )
        }

        @Test
        fun `unnamed enums implement eq and hash`() {
            // TODO Is this test even valid anymore?
            val model = """
                namespace test
                @deprecated
                enum FooEnum {
                    Foo,
                    Bar,
                }
            """.asSmithyModel()
            val shape = model.lookup<StringShape>("test#FooEnum")
            val trait = shape.expectTrait<EnumTrait>()
            val writer = RustWriter.forModule("model")
            writer.rust("##![allow(deprecated)]")
            val generator = EnumGenerator(model, testSymbolProvider(model), writer, shape, trait)
            generator.render()
            writer.compileAndTest(
                """
                assert_eq!(FooEnum::from("Foo"), FooEnum::from("Foo"));
                assert_ne!(FooEnum::from("Bar"), FooEnum::from("Foo"));
                let mut hash_of_enums = std::collections::HashSet::new();
                hash_of_enums.insert(FooEnum::from("Foo"));
                """,
            )
        }

        @Test
        fun `it generates unnamed enums`() {
            // TODO is this test even valid anymore?
            val model = """
                namespace test
                enum FooEnum {
                    Foo,
                    Baz,
                    Bar,
                    1,
                    0
                }
            """.asSmithyModel()
            val shape = model.lookup<StringShape>("test#FooEnum")
            val trait = shape.expectTrait<EnumTrait>()
            val provider = testSymbolProvider(model)
            val writer = RustWriter.forModule("model")
            writer.rust("##![allow(deprecated)]")
            val generator = EnumGenerator(model, provider, writer, shape, trait)
            generator.render()
            writer.compileAndTest(
                """
                // Values should be sorted
                assert_eq!(FooEnum::${EnumGenerator.Values}(), ["0", "1", "Bar", "Baz", "Foo"]);
                """,
            )
        }

        @Test
        fun `it escapes the Unknown variant if the enum has an unknown value in the model`() {
            val model = """
                namespace test
                enum SomeEnum {
                    Known = "Known",
                    Unknown = "Unknown",
                    UnknownValue = "UnknownValue",
                }
            """.asSmithyModel()

            val shape: StringShape = model.lookup("test#SomeEnum")
            val trait = shape.expectTrait<EnumTrait>()
            val provider = testSymbolProvider(model)
            val writer = RustWriter.forModule("model")
            EnumGenerator(model, provider, writer, shape, trait).render()

            writer.compileAndTest(
                """
                assert_eq!(SomeEnum::from("Unknown"), SomeEnum::UnknownValue);
                assert_eq!(SomeEnum::from("UnknownValue"), SomeEnum::UnknownValue_);
                assert_eq!(SomeEnum::from("SomethingNew"), SomeEnum::Unknown("SomethingNew".into()));
                """,
            )
        }

        @Test
        fun `it should generate documentation for enums`() {
            val model = """
                namespace test

                /// Some top-level documentation.
                enum SomeEnum {
                    Known = "Known",
                    Unknown = "Unknown",
                }
            """.asSmithyModel()

            val shape: StringShape = model.lookup("test#SomeEnum")
            val trait = shape.expectTrait<EnumTrait>()
            val provider = testSymbolProvider(model)
            val rendered = RustWriter.forModule("model").also { EnumGenerator(model, provider, it, shape, trait).render() }.toString()

            rendered shouldContain
                """
                /// Some top-level documentation.
                ///
                /// _Note: `SomeEnum::Unknown` has been renamed to `::UnknownValue`._
                """.trimIndent()
        }

        @Test
        fun `it should generate documentation for unnamed enums`() {
            // TODO is this test valid anymore?
            val model = """
                namespace test

                /// Some top-level documentation.
                enum SomeEnum{
                    One,
                    Two,
                }
            """.asSmithyModel()

            val shape: StringShape = model.lookup("test#SomeEnum")
            val trait = shape.expectTrait<EnumTrait>()
            val provider = testSymbolProvider(model)
            val rendered = RustWriter.forModule("model").also { EnumGenerator(model, provider, it, shape, trait).render() }.toString()

            rendered shouldContain
                """
                /// Some top-level documentation.
                """.trimIndent()
        }
    }

    @Test
    fun `it handles variants that clash with Rust reserved words`() {
        val model = """
            namespace test
            enum SomeEnum {
                Known = "Known",
                Self = "other",
            }
        """.asSmithyModel()

        val shape: StringShape = model.lookup("test#SomeEnum")
        val trait = shape.expectTrait<EnumTrait>()
        val provider = testSymbolProvider(model)
        val writer = RustWriter.forModule("model")
        EnumGenerator(model, provider, writer, shape, trait).render()

        writer.compileAndTest(
            """
            assert_eq!(SomeEnum::from("other"), SomeEnum::SelfValue);
            assert_eq!(SomeEnum::from("SomethingNew"), SomeEnum::Unknown("SomethingNew".into()));
            """,
        )
    }
}
