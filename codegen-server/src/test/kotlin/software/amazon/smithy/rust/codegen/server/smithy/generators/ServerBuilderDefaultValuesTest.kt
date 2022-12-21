/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.TestInstance
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.MethodSource
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProvider
import java.util.stream.Stream

@TestInstance(TestInstance.Lifecycle.PER_CLASS)
class ServerBuilderDefaultValuesTest {
    // When defaults are used, the model will be generated with these default values
    private val defaultValues = mapOf(
        "Boolean" to "true",
        "String" to "foo".dq(),
        "Byte" to "5",
        "Short" to "55",
        "Integer" to "555",
        "Long" to "5555",
        "Float" to "0.5",
        "Double" to "0.55",
        "Timestamp" to "1985-04-12T23:20:50.52Z".dq(),
        // "BigInteger" to "55555", "BigDecimal" to "0.555", // TODO(https://github.com/awslabs/smithy-rs/issues/312)
        "StringList" to "[]",
        "IntegerMap" to "{}",
        "Language" to "en".dq(),
        "DocumentBoolean" to "true",
        "DocumentString" to "foo".dq(),
        "DocumentNumberPosInt" to "100",
        "DocumentNumberNegInt" to "-100",
        "DocumentNumberFloat" to "0.1",
        "DocumentList" to "[]",
        "DocumentMap" to "{}",
    )

    // When the test applies values to validate we honor custom values, use these values
    private val customValues =
        mapOf(
            "Boolean" to "false",
            "String" to "bar".dq(),
            "Byte" to "6",
            "Short" to "66",
            "Integer" to "666",
            "Long" to "6666",
            "Float" to "0.6",
            "Double" to "0.66",
            "Timestamp" to "2022-11-25T17:30:50.00Z".dq(),
            // "BigInteger" to "55555", "BigDecimal" to "0.555", // TODO(https://github.com/awslabs/smithy-rs/issues/312)
            "StringList" to "[]",
            "IntegerMap" to "{}",
            "Language" to "fr".dq(),
            "DocumentBoolean" to "false",
            "DocumentString" to "bar".dq(),
            "DocumentNumberPosInt" to "1000",
            "DocumentNumberNegInt" to "-1000",
            "DocumentNumberFloat" to "0.01",
            "DocumentList" to "[]",
            "DocumentMap" to "{}",
        )

    @ParameterizedTest(name = "(#{index}) Generate default value. Required Trait: {1}, nulls: {2}, optionals: {3}")
    @MethodSource("localParameters")
    fun `default values are generated and builders respect default and overrides`(testConfig: TestConfig, setupFiles: (w: RustWriter, m: Model, s: RustSymbolProvider) -> Unit) {
        val initialSetValues = defaultValues.mapValues { if (testConfig.nullDefault) null else it.value }
        val model = generateModel(testConfig, initialSetValues)
        val symbolProvider = serverTestSymbolProvider(model)
        val project = TestWorkspace.testProject(symbolProvider)
        project.withModule(RustModule.public("model")) {
            setupFiles(this, model, symbolProvider)

            val rustValues = setupRustValuesForTest(testConfig.assertValues)
            val applySetters = testConfig.applyDefaultValues
            val setters = if (applySetters) structSetters(rustValues, testConfig.nullDefault && !testConfig.requiredTrait) else writable { }
            val unwrapBuilder = if (testConfig.nullDefault && testConfig.requiredTrait && testConfig.applyDefaultValues) ".unwrap()" else ""
            unitTest(
                name = "generates_default_required_values",
                block = writable {
                    rustTemplate(
                        """
                        let my_struct = MyStruct::builder()
                            #{Setters:W}
                        .build()$unwrapBuilder;

                        #{Assertions:W}
                        """,
                        "Assertions" to assertions(rustValues, applySetters, testConfig.nullDefault, testConfig.requiredTrait, testConfig.applyDefaultValues),
                        "Setters" to setters,
                    )
                },
            )
        }
        project.compileAndTest()
    }

    private fun setupRustValuesForTest(valuesMap: Map<String, String?>): Map<String, String?> {
        return valuesMap + mapOf(
            "Byte" to "${valuesMap["Byte"]}i8",
            "Short" to "${valuesMap["Short"]}i16",
            "Integer" to "${valuesMap["Integer"]}i32",
            "Long" to "${valuesMap["Long"]}i64",
            "Float" to "${valuesMap["Float"]}f32",
            "Double" to "${valuesMap["Double"]}f64",
            "Language" to "crate::model::Language::${valuesMap["Language"]!!.replace(""""""", "").toPascalCase()}",
            "Timestamp" to """aws_smithy_types::DateTime::from_str(${valuesMap["Timestamp"]}, aws_smithy_types::date_time::Format::DateTime).unwrap()""",
            // These must be empty
            "StringList" to "Vec::<String>::new()",
            "IntegerMap" to "std::collections::HashMap::<String, i32>::new()",
            "DocumentList" to "Vec::<aws_smithy_types::Document>::new()",
            "DocumentMap" to "std::collections::HashMap::<String, aws_smithy_types::Document>::new()",
        ) + valuesMap.filter { it.value?.startsWith("Document") == true }.map { it.key to "${it.value}.into()" }
    }

    private fun writeServerBuilderGeneratorWithoutPublicConstrainedTypes(writer: RustWriter, model: Model, symbolProvider: RustSymbolProvider) {
        writer.rust("##![allow(deprecated)]")
        val struct = model.lookup<StructureShape>("com.test#MyStruct")
        val codegenContext = serverTestCodegenContext(model)
        val builderGenerator = ServerBuilderGeneratorWithoutPublicConstrainedTypes(codegenContext, struct)

        writer.implBlock(struct, symbolProvider) {
            builderGenerator.renderConvenienceMethod(writer)
        }
        builderGenerator.render(writer)

        ServerEnumGenerator(codegenContext, writer, model.lookup<EnumShape>("com.test#Language")).render()
        StructureGenerator(model, symbolProvider, writer, struct).render()
    }

    private fun writeServerBuilderGenerator(writer: RustWriter, model: Model, symbolProvider: RustSymbolProvider) {
        writer.rust("##![allow(deprecated)]")
        val struct = model.lookup<StructureShape>("com.test#MyStruct")
        val codegenContext = serverTestCodegenContext(model)
        val builderGenerator = ServerBuilderGenerator(codegenContext, struct)

        writer.implBlock(struct, symbolProvider) {
            builderGenerator.renderConvenienceMethod(writer)
        }
        builderGenerator.render(writer)

        ServerEnumGenerator(codegenContext, writer, model.lookup<EnumShape>("com.test#Language")).render()
        StructureGenerator(model, symbolProvider, writer, struct).render()
    }

    private fun structSetters(values: Map<String, String?>, optional: Boolean): Writable {
        return writable {
            values.entries.forEach {
                rust(".${it.key.toSnakeCase()}(")
                if (optional) {
                    rust("Some(")
                }
                when (it.key) {
                    "String" -> {
                        rust("${it.value}.into()")
                    }

                    "DocumentNull" ->
                        rust("aws_smithy_types::Document::Null")

                    "DocumentString" -> {
                        rust("aws_smithy_types::Document::String(String::from(${it.value}))")
                    }

                    else -> {
                        if (it.key.startsWith("DocumentNumber")) {
                            val type = it.key.replace("DocumentNumber", "")
                            rust("aws_smithy_types::Document::Number(aws_smithy_types::Number::$type(${it.value}))")
                        } else {
                            rust("${it.value}.into()")
                        }
                    }
                }
                if (optional) {
                    rust(")")
                }
                rust(")")
            }
        }
    }

    private fun assertions(values: Map<String, String?>, hasSetValues: Boolean, hasNullValues: Boolean, requiredTrait: Boolean, hasDefaults: Boolean): Writable {
        return writable {
            for (it in values.entries) {
                rust("assert_eq!(my_struct.${it.key.toSnakeCase()} ")
                if (!hasSetValues) {
                    rust(".is_none(), true);")
                    continue
                }

                val expected = if (it.key == "DocumentNull") {
                    "aws_smithy_types::Document::Null"
                } else if (it.key == "DocumentString") {
                    "String::from(${it.value}).into()"
                } else if (it.key.startsWith("DocumentNumber")) {
                    val type = it.key.replace("DocumentNumber", "")
                    "aws_smithy_types::Document::Number(aws_smithy_types::Number::$type(${it.value}))"
                } else if (it.key.startsWith("Document")) {
                    "${it.value}.into()"
                } else {
                    "${it.value}"
                }

                if (!requiredTrait && !(hasDefaults && !hasNullValues)) {
                    rust(".unwrap()")
                }

                rust(", $expected);")
            }
        }
    }

    private fun generateModel(testConfig: TestConfig, values: Map<String, String?>): Model {
        val requiredTrait = if (testConfig.requiredTrait) "@required" else ""

        val members = values.entries.joinToString(", ") {
            val value = if (testConfig.applyDefaultValues) {
                "= ${it.value}"
            } else if (testConfig.nullDefault) {
                "= null"
            } else { "" }
            """
            $requiredTrait
            ${it.key.toPascalCase()}: ${it.key} $value
            """
        }
        val model =
            """
            namespace com.test
            use smithy.framework#ValidationException

            structure MyStruct {
                $members
            }

            enum Language {
                EN = "en",
                FR = "fr",
            }

            list StringList {
                member: String
            }

            map IntegerMap {
                key: String
                value: Integer
            }

            document DocumentNull
            document DocumentBoolean
            document DocumentString
            document DocumentDecimal
            document DocumentNumberNegInt
            document DocumentNumberPosInt
            document DocumentNumberFloat
            document DocumentList
            document DocumentMap
            """
        return model.asSmithyModel(smithyVersion = "2")
    }

    private fun localParameters(): Stream<Arguments> {
        val builderWriters = listOf(
            ::writeServerBuilderGenerator,
            ::writeServerBuilderGeneratorWithoutPublicConstrainedTypes,
        )
        return Stream.of(
            TestConfig(defaultValues, requiredTrait = false, nullDefault = true, applyDefaultValues = true),
            TestConfig(defaultValues, requiredTrait = false, nullDefault = true, applyDefaultValues = false),

            TestConfig(customValues, requiredTrait = false, nullDefault = true, applyDefaultValues = true),
            TestConfig(customValues, requiredTrait = false, nullDefault = true, applyDefaultValues = false),

            TestConfig(defaultValues, requiredTrait = true, nullDefault = true, applyDefaultValues = true),
            TestConfig(customValues, requiredTrait = true, nullDefault = true, applyDefaultValues = true),

            TestConfig(defaultValues, requiredTrait = false, nullDefault = false, applyDefaultValues = true),
            TestConfig(defaultValues, requiredTrait = false, nullDefault = false, applyDefaultValues = false),

            TestConfig(customValues, requiredTrait = false, nullDefault = false, applyDefaultValues = true),
            TestConfig(customValues, requiredTrait = false, nullDefault = false, applyDefaultValues = false),

            TestConfig(defaultValues, requiredTrait = true, nullDefault = false, applyDefaultValues = true),

            TestConfig(customValues, requiredTrait = true, nullDefault = false, applyDefaultValues = true),

        ).flatMap { builderWriters.stream().map { builderWriter -> Arguments.of(it, builderWriter) } }
    }

    data class TestConfig(
        // The values in the setters and assert!() calls
        val assertValues: Map<String, String?>,
        // Whether to apply @required to all members
        val requiredTrait: Boolean,
        // Whether to set all members to `null` and force them to be optional
        val nullDefault: Boolean,
        // Whether to set `assertValues` in the builder
        val applyDefaultValues: Boolean,
    )
}
