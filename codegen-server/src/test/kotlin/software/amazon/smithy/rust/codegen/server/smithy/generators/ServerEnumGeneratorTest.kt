/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.util.expectTrait
import software.amazon.smithy.rust.codegen.util.lookup

class ServerEnumGeneratorTest {
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
                    tags: ["ebsOnly"]
                },
            ])
            string InstanceType
        """.asSmithyModel()
        val provider = testSymbolProvider(model)
        val writer = RustWriter.forModule("model")
        val shape = model.lookup<StringShape>("test#InstanceType")
        val generator = ServerEnumGenerator(model, provider, writer, shape, shape.expectTrait(), TestRuntimeConfig)
        generator.render()
        writer.compileAndTest(
            """
            let instance = InstanceType::T2Micro;
            assert_eq!(instance.as_str(), "t2.micro");
            assert_eq!(InstanceType::try_from("t2.nano").unwrap(), InstanceType::T2Nano);
            // check no unknown
            match instance {
            InstanceType::T2Micro => (),
            InstanceType::T2Nano => (),
            }
            """
        )

        writer.toString() shouldContain "#[non_exhaustive]"
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
        val writer = RustWriter.forModule("model")
        val generator = ServerEnumGenerator(model, testSymbolProvider(model), writer, shape, trait, TestRuntimeConfig)
        generator.render()
        writer.compileAndTest(
            """
            assert_eq!(FooEnum::Foo, FooEnum::Foo);
            assert_ne!(FooEnum::Bar, FooEnum::Foo);
            let mut hash_of_enums = std::collections::HashSet::new();
            hash_of_enums.insert(FooEnum::Foo);
            """.trimIndent()
        )
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
            string FooEnum
        """.asSmithyModel()
        val shape = model.lookup<StringShape>("test#FooEnum")
        val trait = shape.expectTrait<EnumTrait>()
        val writer = RustWriter.forModule("model")
        val generator = ServerEnumGenerator(model, testSymbolProvider(model), writer, shape, trait, TestRuntimeConfig)
        generator.render()
        writer.compileAndTest(
            """
            assert_eq!(FooEnum::try_from("Foo").unwrap(), FooEnum::try_from("Foo").unwrap());
            assert_ne!(FooEnum::try_from("Bar").unwrap(), FooEnum::try_from("Foo").unwrap());
            let mut hash_set_of_enums = std::collections::HashSet::new();
            hash_set_of_enums.insert(FooEnum::try_from("Foo").unwrap());
            """
        )
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

        val shape: StringShape = model.lookup("test#SomeEnum")
        val trait = shape.expectTrait<EnumTrait>()
        val provider = testSymbolProvider(model)
        val writer = RustWriter.forModule("model")
        ServerEnumGenerator(model, provider, writer, shape, trait, TestRuntimeConfig).render()

        writer.compileAndTest(
            """
            assert_eq!(SomeEnum::try_from("Unknown").unwrap(), SomeEnum::UnknownValue);
            assert_eq!(SomeEnum::try_from("UnknownValue").unwrap(), SomeEnum::UnknownValue_);
            assert_eq!(SomeEnum::try_from("SomethingNew").is_err(), true);
            """
        )
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

        val shape: StringShape = model.lookup("test#SomeEnum")
        val trait = shape.expectTrait<EnumTrait>()
        val provider = testSymbolProvider(model)
        val rendered = RustWriter.forModule("model").also { ServerEnumGenerator(model, provider, it, shape, trait, TestRuntimeConfig).render() }.toString()

        rendered shouldContain
            """
            /// Some top-level documentation.
            ///
            /// _Note: `SomeEnum::Unknown` has been renamed to `::UnknownValue`._
            """.trimIndent()
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

        val shape: StringShape = model.lookup("test#SomeEnum")
        val trait = shape.expectTrait<EnumTrait>()
        val provider = testSymbolProvider(model)
        val rendered = RustWriter.forModule("model").also { ServerEnumGenerator(model, provider, it, shape, trait, TestRuntimeConfig).render() }.toString()

        rendered shouldContain
            """
            /// Some top-level documentation.
            """.trimIndent()
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

        val shape: StringShape = model.lookup("test#SomeEnum")
        val trait = shape.expectTrait<EnumTrait>()
        val provider = testSymbolProvider(model)
        val writer = RustWriter.forModule("model")
        ServerEnumGenerator(model, provider, writer, shape, trait, TestRuntimeConfig).render()

        writer.compileAndTest(
            """
            assert_eq!(SomeEnum::try_from("other").unwrap(), SomeEnum::SelfValue);
            assert_eq!(SomeEnum::try_from("SomethingNew").is_err(), true);
            """
        )
    }
}
