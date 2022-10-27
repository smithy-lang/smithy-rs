/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.makeMaybeConstrained
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.redactIfNecessary
import software.amazon.smithy.rust.codegen.server.smithy.PubCrateConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
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

    fun render() {
        val lengthTrait = shape.expectTrait<LengthTrait>()

        val symbol = constrainedShapeSymbolProvider.toSymbol(shape)
        val name = symbol.name
        val inner = RustType.String.render()
        val constraintViolation = constraintViolationSymbolProvider.toSymbol(shape)

        val condition = if (lengthTrait.min.isPresent && lengthTrait.max.isPresent) {
            "(${lengthTrait.min.get()}..=${lengthTrait.max.get()}).contains(&length)"
        } else if (lengthTrait.min.isPresent) {
            "${lengthTrait.min.get()} <= length"
        } else {
            "length <= ${lengthTrait.max.get()}"
        }

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
        writer.rustTemplate(
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
            }
            
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
            
            impl #{TryFrom}<$inner> for $name {
                type Error = #{ConstraintViolation};
                
                /// ${rustDocsTryFromMethod(name, inner)}
                fn try_from(value: $inner) -> Result<Self, Self::Error> {
                    let length = value.chars().count();
                    if $condition {
                        Ok(Self(value))
                    } else {
                        Err(#{ConstraintViolation}::Length(length))
                    }
                }
            }
            
            impl #{From}<$name> for $inner {
                fn from(value: $name) -> Self {
                    value.into_inner()
                }
            }
            """,
            "ConstrainedTrait" to RuntimeType.ConstrainedTrait(),
            "ConstraintViolation" to constraintViolation,
            "MaybeConstrained" to symbol.makeMaybeConstrained(),
            "Display" to RuntimeType.Display,
            "From" to RuntimeType.From,
            "TryFrom" to RuntimeType.TryFrom,
        )

        val constraintViolationModuleName = constraintViolation.namespace.split(constraintViolation.namespaceDelimiter).last()
        writer.withModule(RustModule(constraintViolationModuleName, RustMetadata(visibility = constrainedTypeVisibility))) {
            rust(
                """
                ##[derive(Debug, PartialEq)]
                pub enum ${constraintViolation.name} {
                    Length(usize),
                }
                """,
            )

            if (shape.isReachableFromOperationInput()) {
                rustBlock("impl ${constraintViolation.name}") {
                    rustBlockTemplate(
                        "pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::ValidationExceptionField",
                        "String" to RuntimeType.String,
                    ) {
                        rustBlock("match self") {
                            rust(
                                """
                                Self::Length(length) => crate::model::ValidationExceptionField {
                                    message: format!("${lengthTrait.validationErrorMessage()}", length, &path),
                                    path,
                                },
                                """,
                            )
                        }
                    }
                }
            }
        }
    }
}
