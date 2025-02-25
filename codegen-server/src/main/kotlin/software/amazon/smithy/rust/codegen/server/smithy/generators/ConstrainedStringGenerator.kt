/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.model.traits.PatternTrait
import software.amazon.smithy.model.traits.SensitiveTrait
import software.amazon.smithy.model.traits.Trait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.smithy.makeMaybeConstrained
import software.amazon.smithy.rust.codegen.core.smithy.testModuleForShape
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.redactIfNecessary
import software.amazon.smithy.rust.codegen.server.smithy.InlineModuleCreator
import software.amazon.smithy.rust.codegen.server.smithy.PubCrateConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.shapeConstraintViolationDisplayMessage
import software.amazon.smithy.rust.codegen.server.smithy.supportedStringConstraintTraits
import software.amazon.smithy.rust.codegen.server.smithy.traits.isReachableFromOperationInput
import software.amazon.smithy.rust.codegen.server.smithy.validationErrorMessage

/**
 * [ConstrainedStringGenerator] generates a wrapper tuple newtype holding a constrained `String`.
 * This type can be built from unconstrained values, yielding a `ConstraintViolation` when the input does not satisfy
 * the constraints.
 */
class ConstrainedStringGenerator(
    val codegenContext: ServerCodegenContext,
    private val inlineModuleCreator: InlineModuleCreator,
    private val writer: RustWriter,
    val shape: StringShape,
    private val validationExceptionConversionGenerator: ValidationExceptionConversionGenerator,
) {
    val model = codegenContext.model
    val constrainedShapeSymbolProvider = codegenContext.constrainedShapeSymbolProvider
    val publicConstrainedTypes = codegenContext.settings.codegenConfig.publicConstrainedTypes
    private val constraintViolationSymbolProvider =
        with(codegenContext.constraintViolationSymbolProvider) {
            if (publicConstrainedTypes) {
                this
            } else {
                PubCrateConstraintViolationSymbolProvider(this)
            }
        }
    private val symbol = constrainedShapeSymbolProvider.toSymbol(shape)
    private val stringConstraintsInfo: List<StringTraitInfo> =
        supportedStringConstraintTraits
            .mapNotNull { shape.getTrait(it).orNull() }
            .map { StringTraitInfo.fromTrait(symbol, it, isSensitive = shape.hasTrait<SensitiveTrait>()) }
    private val constraintsInfo: List<TraitInfo> =
        stringConstraintsInfo
            .map(StringTraitInfo::toTraitInfo)

    fun render() {
        val name = symbol.name
        val inner = RustType.String.render()
        val constraintViolation = constraintViolationSymbolProvider.toSymbol(shape)

        writer.documentShape(shape, model)
        writer.docs(rustDocsConstrainedTypeEpilogue(name))
        val metadata = symbol.expectRustMetadata()
        metadata.render(writer)
        writer.rust("struct $name(pub(crate) $inner);")
        if (metadata.visibility == Visibility.PUBCRATE) {
            Attribute.AllowDeadCode.render(writer)
        }
        writer.rust(
            """
            impl $name {
                /// Extracts a string slice containing the entire underlying `String`.
                pub fn as_str(&self) -> &str {
                    &self.0
                }

                /// ${rustDocsInnerMethod(inner)}
                pub fn inner(&self) -> &$inner {
                    &self.0
                }

                /// ${rustDocsIntoInnerMethod(inner)}
                pub fn into_inner(self) -> $inner {
                    self.0
                }
            }""",
        )

        writer.renderTryFrom(inner, name, constraintViolation, constraintsInfo)

        writer.rustTemplate(
            """
            impl #{ConstrainedTrait} for $name  {
                type Unconstrained = $inner;
            }

            impl #{From}<$inner> for #{MaybeConstrained} {
                fn from(value: $inner) -> Self {
                    Self::Unconstrained(value)
                }
            }

            impl #{Display} for $name {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                   ${shape.redactIfNecessary(model, "self.0")}.fmt(f)
                }
            }

            impl #{From}<$name> for $inner {
                fn from(value: $name) -> Self {
                    value.into_inner()
                }
            }
            """,
            "ConstrainedTrait" to RuntimeType.ConstrainedTrait,
            "ConstraintViolation" to constraintViolation,
            "MaybeConstrained" to symbol.makeMaybeConstrained(),
            "Display" to RuntimeType.Display,
            "From" to RuntimeType.From,
        )

        inlineModuleCreator(constraintViolation) {
            renderConstraintViolationEnum(this, shape, constraintViolation)
        }

        renderTests(shape)
    }

    private fun renderConstraintViolationEnum(
        writer: RustWriter,
        shape: StringShape,
        constraintViolation: Symbol,
    ) {
        writer.rustTemplate(
            """
            ##[derive(Debug, PartialEq)]
            pub enum ${constraintViolation.name} {
                #{Variants:W}
            }

            impl #{Display} for ${constraintViolation.name} {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    let message = match self {
                        #{VariantDisplayMessages:W}
                    };
                    write!(f, "{message}")
                }
            }

            impl #{Error} for ${constraintViolation.name} {}
            """,
            "Variants" to constraintsInfo.map { it.constraintViolationVariant }.join(",\n"),
            "Error" to RuntimeType.StdError,
            "Display" to RuntimeType.Display,
            "VariantDisplayMessages" to generateDisplayMessageForEachVariant(),
        )

        if (shape.isReachableFromOperationInput()) {
            writer.rustTemplate(
                """
                impl ${constraintViolation.name} {
                    #{StringShapeConstraintViolationImplBlock:W}
                }
                """,
                "StringShapeConstraintViolationImplBlock" to validationExceptionConversionGenerator.stringShapeConstraintViolationImplBlock(stringConstraintsInfo),
            )
        }
    }

    private fun generateDisplayMessageForEachVariant() =
        writable {
            stringConstraintsInfo.forEach {
                it.shapeConstraintViolationDisplayMessage(shape).invoke(this)
            }
        }

    private fun renderTests(shape: Shape) {
        val testCases = TraitInfo.testCases(constraintsInfo)

        if (testCases.isNotEmpty()) {
            val testModule = constrainedShapeSymbolProvider.testModuleForShape(shape)
            writer.withInlineModule(testModule, null) {
                rustTemplate(
                    """
                    #{TestCases:W}
                    """,
                    "TestCases" to testCases.join("\n"),
                )
            }
        }
    }
}

data class Length(val lengthTrait: LengthTrait) : StringTraitInfo() {
    override fun toTraitInfo(): TraitInfo =
        TraitInfo(
            tryFromCheck = { rust("Self::check_length(&value)?;") },
            constraintViolationVariant = {
                docs("Error when a string doesn't satisfy its `@length` requirements.")
                rust("Length(usize)")
            },
            asValidationExceptionField = {
                rust(
                    """
                    Self::Length(length) => crate::model::ValidationExceptionField {
                        message: format!("${lengthTrait.validationErrorMessage()}", length, &path),
                        path,
                    },
                    """,
                )
            },
            validationFunctionDefinition = this::renderValidationFunction,
        )

    /**
     * Renders a `check_length` function to validate the string matches the
     * required length indicated by the `@length` trait.
     */
    @Suppress("UNUSED_PARAMETER")
    private fun renderValidationFunction(
        constraintViolation: Symbol,
        unconstrainedTypeName: String,
    ): Writable =
        {
            // Note that we're using the linear time check `chars().count()` instead of `len()` on the input value, since the
            // Smithy specification says the `length` trait counts the number of Unicode code points when applied to string shapes.
            // https://awslabs.github.io/smithy/1.0/spec/core/constraint-traits.html#length-trait
            rustTemplate(
                """
                fn check_length(string: &str) -> #{Result}<(), $constraintViolation> {
                    let length = string.chars().count();

                    if ${lengthTrait.rustCondition("length")} {
                        Ok(())
                    } else {
                        Err($constraintViolation::Length(length))
                    }
                }
                """,
                *preludeScope,
            )
        }

    override fun shapeConstraintViolationDisplayMessage(shape: Shape) =
        writable {
            rustTemplate(
                """
                Self::Length(length) => {
                    format!("${lengthTrait.shapeConstraintViolationDisplayMessage(shape).replace("#", "##")}", length)
                },
                """,
            )
        }
}

data class Pattern(val symbol: Symbol, val patternTrait: PatternTrait, val isSensitive: Boolean) : StringTraitInfo() {
    override fun toTraitInfo(): TraitInfo {
        return TraitInfo(
            tryFromCheck = { rust("let value = Self::check_pattern(value)?;") },
            constraintViolationVariant = {
                docs("Error when a string doesn't satisfy its `@pattern`.")
                docs("Contains the String that failed the pattern.")
                rust("Pattern(String)")
            },
            asValidationExceptionField = {
                Attribute.AllowUnusedVariables.render(this)
                rustTemplate(
                    """
                    Self::Pattern(_) => crate::model::ValidationExceptionField {
                        message: #{ErrorMessage:W},
                        path
                    },
                    """,
                    "ErrorMessage" to errorMessage(),
                )
            },
            this::renderValidationFunction,
            testCases =
                listOf {
                    unitTest("regex_compiles") {
                        rustTemplate(
                            """
                            #{T}::compile_regex();
                            """,
                            "T" to symbol,
                        )
                    }
                },
        )
    }

    fun errorMessage(): Writable {
        return writable {
            val pattern = patternTrait.pattern.toString().replace("#", "##")
            rust(
                """
                format!("${patternTrait.validationErrorMessage()}", &path, r##"$pattern"##)
                """,
            )
        }
    }

    /**
     * Renders a `check_pattern` function to validate the string matches the
     * supplied regex in the `@pattern` trait.
     */
    private fun renderValidationFunction(
        constraintViolation: Symbol,
        unconstrainedTypeName: String,
    ): Writable {
        val pattern = patternTrait.pattern.toString().replace("#", "##")
        val errorMessageForUnsupportedRegex =
            """The regular expression $pattern is not supported by the `regex` crate; feel free to file an issue under https://github.com/smithy-lang/smithy-rs/issues for support"""

        return {
            rustTemplate(
                """
                fn check_pattern(string: $unconstrainedTypeName) -> #{Result}<$unconstrainedTypeName, $constraintViolation> {
                    let regex = Self::compile_regex();

                    if regex.is_match(&string) {
                        Ok(string)
                    } else {
                        Err($constraintViolation::Pattern(string))
                    }
                }

                /// Attempts to compile the regex for this constrained type's `@pattern`.
                /// This can fail if the specified regex is not supported by the `#{Regex}` crate.
                pub fn compile_regex() -> &'static #{Regex}::Regex {
                    static REGEX: #{OnceCell}::sync::Lazy<#{Regex}::Regex> = #{OnceCell}::sync::Lazy::new(|| #{Regex}::Regex::new(r##"$pattern"##).expect(r##"$errorMessageForUnsupportedRegex"##));

                    &REGEX
                }
                """,
                "Regex" to ServerCargoDependency.Regex.toType(),
                "OnceCell" to ServerCargoDependency.OnceCell.toType(),
                *preludeScope,
            )
        }
    }

    override fun shapeConstraintViolationDisplayMessage(shape: Shape) =
        writable {
            val errorMessage = patternTrait.shapeConstraintViolationDisplayMessage(shape).replace("#", "##")
            val pattern = patternTrait.pattern.toString().replace("#", "##")
            rustTemplate(
                """
                Self::Pattern(_) => {
                    format!(r##"$errorMessage"##, r##"$pattern"##)
                },
                """,
            )
        }
}

sealed class StringTraitInfo {
    companion object {
        fun fromTrait(
            symbol: Symbol,
            trait: Trait,
            isSensitive: Boolean,
        ) = when (trait) {
            is PatternTrait -> {
                Pattern(symbol, trait, isSensitive)
            }

            is LengthTrait -> {
                Length(trait)
            }

            else -> PANIC("StringTraitInfo.fromTrait called with unsupported trait $trait")
        }
    }

    abstract fun toTraitInfo(): TraitInfo

    abstract fun shapeConstraintViolationDisplayMessage(shape: Shape): Writable
}
