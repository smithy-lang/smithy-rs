/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.model.traits.PatternTrait
import software.amazon.smithy.model.traits.Trait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustMetadata
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
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.makeMaybeConstrained
import software.amazon.smithy.rust.codegen.core.smithy.module
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.redactIfNecessary
import software.amazon.smithy.rust.codegen.server.smithy.PubCrateConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
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
    val writer: RustWriter,
    val shape: StringShape,
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
    private val constraintsInfo: List<TraitInfo> =
        supportedStringConstraintTraits
            .mapNotNull { shape.getTrait(it).orNull() }
            .map(StringTraitInfo::fromTrait)
            .map(StringTraitInfo::toTraitInfo)

    fun render() {
        val symbol = constrainedShapeSymbolProvider.toSymbol(shape)
        val name = symbol.name
        val inner = RustType.String.render()
        val constraintViolation = constraintViolationSymbolProvider.toSymbol(shape)

        val constrainedTypeVisibility = if (publicConstrainedTypes) {
            Visibility.PUBLIC
        } else {
            Visibility.PUBCRATE
        }
        val constrainedTypeMetadata = RustMetadata(
            Attribute.Derives(setOf(RuntimeType.Debug, RuntimeType.Clone, RuntimeType.PartialEq, RuntimeType.Eq, RuntimeType.Hash)),
            visibility = constrainedTypeVisibility,
        )

        // Note that we're using the linear time check `chars().count()` instead of `len()` on the input value, since the
        // Smithy specification says the `length` trait counts the number of Unicode code points when applied to string shapes.
        // https://awslabs.github.io/smithy/1.0/spec/core/constraint-traits.html#length-trait
        writer.documentShape(shape, model, note = rustDocsNote(name))
        constrainedTypeMetadata.render(writer)
        writer.rust("struct $name(pub(crate) $inner);")
        if (constrainedTypeVisibility == Visibility.PUBCRATE) {
            Attribute.AllowUnused.render(writer)
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

        writer.withInlineModule(constraintViolation.module()) {
            renderConstraintViolationEnum(this, shape, constraintViolation)
        }
    }

    private fun renderConstraintViolationEnum(writer: RustWriter, shape: StringShape, constraintViolation: Symbol) {
        writer.rustTemplate(
            """
            ##[derive(Debug, PartialEq)]
            pub enum ${constraintViolation.name} {
              #{Variants:W}
            }
            """,
            "Variants" to constraintsInfo.map { it.constraintViolationVariant }.join(",\n"),
        )

        if (shape.isReachableFromOperationInput()) {
            writer.rustTemplate(
                """
                impl ${constraintViolation.name} {
                    pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::ValidationExceptionField {
                        match self {
                            #{ValidationExceptionFields:W}
                        }
                    }
                }
                """,
                "String" to RuntimeType.String,
                "ValidationExceptionFields" to constraintsInfo.map { it.asValidationExceptionField }.join("\n"),
            )
        }
    }
}
private data class Length(val lengthTrait: LengthTrait) : StringTraitInfo() {
    override fun toTraitInfo(): TraitInfo = TraitInfo(
        { rust("Self::check_length(&value)?;") },
        {
            docs("Error when a string doesn't satisfy its `@length` requirements.")
            rust("Length(usize)")
        },
        {
            rust(
                """
                Self::Length(length) => crate::model::ValidationExceptionField {
                    message: format!("${lengthTrait.validationErrorMessage()}", length, &path),
                    path,
                },
                """,
            )
        },
        this::renderValidationFunction,
    )

    /**
     * Renders a `check_length` function to validate the string matches the
     * required length indicated by the `@length` trait.
     */
    @Suppress("UNUSED_PARAMETER")
    private fun renderValidationFunction(constraintViolation: Symbol, unconstrainedTypeName: String): Writable = {
        rust(
            """
            fn check_length(string: &str) -> Result<(), $constraintViolation> {
                let length = string.chars().count();

                if ${lengthTrait.rustCondition("length")} {
                    Ok(())
                } else {
                    Err($constraintViolation::Length(length))
                }
            }
            """,
        )
    }
}

private data class Pattern(val patternTrait: PatternTrait) : StringTraitInfo() {
    override fun toTraitInfo(): TraitInfo {
        val pattern = patternTrait.pattern

        return TraitInfo(
            { rust("let value = Self::check_pattern(value)?;") },
            {
                docs("Error when a string doesn't satisfy its `@pattern`.")
                docs("Contains the String that failed the pattern.")
                rust("Pattern(String)")
            },
            {
                rust(
                    """
                    Self::Pattern(string) => crate::model::ValidationExceptionField {
                        message: format!("${patternTrait.validationErrorMessage()}", &string, &path, r##"$pattern"##),
                        path
                    },
                    """,
                )
            },
            this::renderValidationFunction,
        )
    }

    /**
     * Renders a `check_pattern` function to validate the string matches the
     * supplied regex in the `@pattern` trait.
     */
    private fun renderValidationFunction(constraintViolation: Symbol, unconstrainedTypeName: String): Writable {
        val pattern = patternTrait.pattern
        val errorMessageForUnsupportedRegex =
            """The regular expression $pattern is not supported by the `regex` crate; feel free to file an issue under https://github.com/awslabs/smithy-rs/issues for support"""

        return {
            rustTemplate(
                """
                fn check_pattern(string: $unconstrainedTypeName) -> Result<$unconstrainedTypeName, $constraintViolation> {
                    let regex = Self::compile_regex();

                    if regex.is_match(&string) {
                        Ok(string)
                    } else {
                        Err($constraintViolation::Pattern(string))
                    }
                }

                pub fn compile_regex() -> &'static #{Regex}::Regex {
                    static REGEX: #{OnceCell}::sync::Lazy<#{Regex}::Regex> = #{OnceCell}::sync::Lazy::new(|| #{Regex}::Regex::new(r##"$pattern"##).expect(r##"$errorMessageForUnsupportedRegex"##));

                    &REGEX
                }
                """,
                "Regex" to ServerCargoDependency.Regex.toType(),
                "OnceCell" to ServerCargoDependency.OnceCell.toType(),
            )
        }
    }
}

private sealed class StringTraitInfo {
    companion object {
        fun fromTrait(trait: Trait): StringTraitInfo =
            when (trait) {
                is PatternTrait -> {
                    Pattern(trait)
                }
                is LengthTrait -> {
                    Length(trait)
                }
                else -> PANIC("StringTraitInfo.fromTrait called with unsupported trait $trait")
            }
    }

    abstract fun toTraitInfo(): TraitInfo
}
