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
import software.amazon.smithy.rust.codegen.core.rustlang.conditionalBlock
import software.amazon.smithy.rust.codegen.core.rustlang.implBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenConfig
import software.amazon.smithy.rust.codegen.server.smithy.customizations.SmithyValidationExceptionConversionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerRestJsonProtocol
import software.amazon.smithy.rust.codegen.server.smithy.renderInlineMemoryModules
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestRustSettings
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProvider
import java.util.stream.Stream

@TestInstance(TestInstance.Lifecycle.PER_CLASS)
class ServerBuilderDefaultValuesTest {
    // When defaults are used, the model will be generated with these in the `@default` trait.
    private val defaultValues =
        mapOf(
            "Boolean" to "true",
            "String" to "foo".dq(),
            "Byte" to "5",
            "Short" to "55",
            "Integer" to "555",
            "Long" to "5555",
            "Float" to "0.5",
            "Double" to "0.55",
            "Timestamp" to "1985-04-12T23:20:50.52Z".dq(),
            // "BigInteger" to "55555", "BigDecimal" to "0.555", // TODO(https://github.com/smithy-lang/smithy-rs/issues/312)
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

    // When the test applies values to validate we honor custom values, use these (different) values.
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
            // "BigInteger" to "55555", "BigDecimal" to "0.555", // TODO(https://github.com/smithy-lang/smithy-rs/issues/312)
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

    @ParameterizedTest(name = "(#{index}) Server builders and default values. Params = requiredTrait: {0}, nullDefault: {1}, applyDefaultValues: {2}, builderGeneratorKind: {3}, assertValues: {4}")
    @MethodSource("testParameters")
    fun `default values are generated and builders respect default and overrides`(
        requiredTrait: Boolean,
        nullDefault: Boolean,
        applyDefaultValues: Boolean,
        builderGeneratorKind: BuilderGeneratorKind,
        assertValues: Map<String, String?>,
    ) {
        println("Running test with params = requiredTrait: $requiredTrait, nullDefault: $nullDefault, applyDefaultValues: $applyDefaultValues, builderGeneratorKind: $builderGeneratorKind, assertValues: $assertValues")
        val initialSetValues = this.defaultValues.mapValues { if (nullDefault) null else it.value }
        val model = generateModel(requiredTrait, applyDefaultValues, nullDefault, initialSetValues)
        val symbolProvider = serverTestSymbolProvider(model)
        val project = TestWorkspace.testProject(symbolProvider)

        project.withModule(RustModule.public("model")) {
            when (builderGeneratorKind) {
                BuilderGeneratorKind.SERVER_BUILDER_GENERATOR -> {
                    writeServerBuilderGenerator(project, this, model, symbolProvider)
                }

                BuilderGeneratorKind.SERVER_BUILDER_GENERATOR_WITHOUT_PUBLIC_CONSTRAINED_TYPES -> {
                    writeServerBuilderGeneratorWithoutPublicConstrainedTypes(project, this, model, symbolProvider)
                }
            }

            val rustValues = setupRustValuesForTest(assertValues)
            val setters =
                if (applyDefaultValues) {
                    structSetters(rustValues, nullDefault && !requiredTrait)
                } else {
                    writable { }
                }
            val unwrapBuilder = if (nullDefault && requiredTrait && applyDefaultValues) ".unwrap()" else ""
            unitTest(
                name = "generates_default_required_values",
                block =
                    writable {
                        rustTemplate(
                            """
                            let my_struct = MyStruct::builder()
                                #{Setters:W}
                                .build()
                                $unwrapBuilder;

                            #{Assertions:W}
                            """,
                            "Assertions" to
                                assertions(
                                    rustValues,
                                    applyDefaultValues,
                                    nullDefault,
                                    requiredTrait,
                                    applyDefaultValues,
                                ),
                            "Setters" to setters,
                        )
                    },
            )
        }

        project.renderInlineMemoryModules()
        // Run clippy because the builder's code for handling `@default` is prone to upset it.
        project.compileAndTest(runClippy = true)
    }

    private fun setupRustValuesForTest(valuesMap: Map<String, String?>): Map<String, String?> {
        return valuesMap +
            mapOf(
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
            ) +
            valuesMap
                .filter { it.value?.startsWith("Document") ?: false }
                .map { it.key to "${it.value}.into()" }
    }

    private fun writeServerBuilderGeneratorWithoutPublicConstrainedTypes(
        rustCrate: RustCrate,
        writer: RustWriter,
        model: Model,
        symbolProvider: RustSymbolProvider,
    ) {
        val struct = model.lookup<StructureShape>("com.test#MyStruct")
        val codegenContext =
            serverTestCodegenContext(
                model,
                settings =
                    serverTestRustSettings(
                        codegenConfig = ServerCodegenConfig(publicConstrainedTypes = false),
                    ),
            )
        val builderGenerator =
            ServerBuilderGeneratorWithoutPublicConstrainedTypes(
                codegenContext,
                struct,
                SmithyValidationExceptionConversionGenerator(codegenContext),
                ServerRestJsonProtocol(codegenContext),
            )

        writer.implBlock(symbolProvider.toSymbol(struct)) {
            builderGenerator.renderConvenienceMethod(writer)
        }
        builderGenerator.render(rustCrate, writer)

        ServerEnumGenerator(
            codegenContext,
            model.lookup<EnumShape>("com.test#Language"),
            SmithyValidationExceptionConversionGenerator(codegenContext),
            emptyList(),
        ).render(writer)
        StructureGenerator(model, symbolProvider, writer, struct, emptyList(), codegenContext.structSettings()).render()
    }

    private fun writeServerBuilderGenerator(
        rustCrate: RustCrate,
        writer: RustWriter,
        model: Model,
        symbolProvider: RustSymbolProvider,
    ) {
        val struct = model.lookup<StructureShape>("com.test#MyStruct")
        val codegenContext = serverTestCodegenContext(model)
        val builderGenerator =
            ServerBuilderGenerator(
                codegenContext,
                struct,
                SmithyValidationExceptionConversionGenerator(codegenContext),
                ServerRestJsonProtocol(codegenContext),
            )

        writer.implBlock(symbolProvider.toSymbol(struct)) {
            builderGenerator.renderConvenienceMethod(writer)
        }
        builderGenerator.render(rustCrate, writer)

        ServerEnumGenerator(
            codegenContext,
            model.lookup<EnumShape>("com.test#Language"),
            SmithyValidationExceptionConversionGenerator(codegenContext),
            emptyList(),
        ).render(writer)
        StructureGenerator(model, symbolProvider, writer, struct, emptyList(), codegenContext.structSettings()).render()
    }

    private fun structSetters(
        values: Map<String, String?>,
        optional: Boolean,
    ) = writable {
        for ((key, value) in values) {
            withBlock(".${key.toSnakeCase()}(", ")") {
                conditionalBlock("Some(", ")", optional) {
                    when (key) {
                        "String" -> rust("$value.into()")
                        "DocumentNull" -> rust("aws_smithy_types::Document::Null")
                        "DocumentString" -> rust("aws_smithy_types::Document::String(String::from($value))")

                        else -> {
                            if (key.startsWith("DocumentNumber")) {
                                val type = key.replace("DocumentNumber", "")
                                rust("aws_smithy_types::Document::Number(aws_smithy_types::Number::$type($value))")
                            } else {
                                rust("$value.into()")
                            }
                        }
                    }
                }
            }
        }
    }

    private fun assertions(
        values: Map<String, String?>,
        hasSetValues: Boolean,
        hasNullValues: Boolean,
        requiredTrait: Boolean,
        hasDefaults: Boolean,
    ) = writable {
        for ((key, value) in values) {
            val member = "my_struct.${key.toSnakeCase()}"

            if (!hasSetValues) {
                rust("assert!($member.is_none());")
            } else {
                val actual =
                    writable {
                        rust(member)
                        if (!requiredTrait && !(hasDefaults && !hasNullValues)) {
                            rust(".unwrap()")
                        }
                    }
                val expected =
                    writable {
                        val expected =
                            if (key == "DocumentNull") {
                                "aws_smithy_types::Document::Null"
                            } else if (key == "DocumentString") {
                                "String::from($value).into()"
                            } else if (key.startsWith("DocumentNumber")) {
                                val type = key.replace("DocumentNumber", "")
                                "aws_smithy_types::Document::Number(aws_smithy_types::Number::$type($value))"
                            } else if (key.startsWith("Document")) {
                                "$value.into()"
                            } else {
                                "$value"
                            }
                        rust(expected)
                    }
                rustTemplate("assert_eq!(#{Actual:W}, #{Expected:W});", "Actual" to actual, "Expected" to expected)
            }
        }
    }

    private fun generateModel(
        requiredTrait: Boolean,
        applyDefaultValues: Boolean,
        nullDefault: Boolean,
        values: Map<String, String?>,
    ): Model {
        val requiredOrNot = if (requiredTrait) "@required" else ""

        val members =
            values.entries.joinToString(", ") {
                val value =
                    if (applyDefaultValues) {
                        "= ${it.value}"
                    } else if (nullDefault) {
                        "= null"
                    } else {
                        ""
                    }
                """
                $requiredOrNot
                ${it.key.toPascalCase()}: ${it.key} $value
                """
            }
        val model =
            """
            namespace com.test

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

    /**
     * The builder generator we should test.
     * We use an enum instead of directly passing in the closure so that JUnit can print a helpful string in the test
     * report.
     */
    enum class BuilderGeneratorKind {
        SERVER_BUILDER_GENERATOR,
        SERVER_BUILDER_GENERATOR_WITHOUT_PUBLIC_CONSTRAINED_TYPES,
    }

    private fun testParameters(): Stream<Arguments> {
        val builderGeneratorKindList =
            listOf(
                BuilderGeneratorKind.SERVER_BUILDER_GENERATOR,
                BuilderGeneratorKind.SERVER_BUILDER_GENERATOR_WITHOUT_PUBLIC_CONSTRAINED_TYPES,
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
        ).flatMap { (assertValues, requiredTrait, nullDefault, applyDefaultValues) ->
            builderGeneratorKindList.stream().map { builderGeneratorKind ->
                Arguments.of(requiredTrait, nullDefault, applyDefaultValues, builderGeneratorKind, assertValues)
            }
        }
    }

    data class TestConfig(
        // The values in the `assert!()` calls and for the `@default` trait
        val assertValues: Map<String, String?>,
        // Whether to apply @required to all members
        val requiredTrait: Boolean,
        // Whether to set all members to `null` and force them to be optional
        val nullDefault: Boolean,
        // Whether to set `assertValues` in the builder
        val applyDefaultValues: Boolean,
    )
}
