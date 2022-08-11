/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Visibility
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.smithy.ConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.util.expectTrait

/**
 * [ConstrainedMapGenerator] generates a wrapper tuple newtype holding a constrained `std::collections::HashMap`.
 * This type can be built from unconstrained values, yielding a `ConstraintViolation` when the input does not satisfy
 * the constraints.
 *
 * The [`length` trait] is the only constraint trait applicable to map shapes.
 *
 * If [unconstrainedSymbol] is provided, the `MaybeConstrained` trait is implemented for the constrained type, using the
 * [unconstrainedSymbol]'s associated type as the associated type for the trait.
 *
 * [`length` trait]: https://awslabs.github.io/smithy/1.0/spec/core/constraint-traits.html#length-trait
 */
class ConstrainedMapGenerator(
    val model: Model,
    private val constrainedSymbolProvider: RustSymbolProvider,
    private val constraintViolationSymbolProvider: ConstraintViolationSymbolProvider,
    private val publicConstrainedTypes: Boolean,
    private val symbolProvider: RustSymbolProvider,
    val writer: RustWriter,
    val shape: MapShape,
    private val unconstrainedSymbol: Symbol? = null,
) {
    fun render() {
        // The `length` trait is the only constraint trait applicable to map shapes.
        val lengthTrait = shape.expectTrait<LengthTrait>()

        val name = constrainedSymbolProvider.toSymbol(shape).name
        val inner = "std::collections::HashMap<#{KeySymbol}, #{ValueSymbol}>"
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
            Attribute.Derives(setOf(RuntimeType.Debug, RuntimeType.Clone, RuntimeType.PartialEq)),
            visibility = constrainedTypeVisibility
        )

        val codegenScope = arrayOf(
            "KeySymbol" to constrainedSymbolProvider.toSymbol(model.expectShape(shape.key.target)),
            "ValueSymbol" to constrainedSymbolProvider.toSymbol(model.expectShape(shape.value.target)),
            "TryFrom" to RuntimeType.TryFrom,
            "ConstraintViolation" to constraintViolation
        )

        // TODO Display impl missing; it should honor `sensitive` trait.

        writer.documentShape(shape, model, note = rustDocsNote(name))
        constrainedTypeMetadata.render(writer)
        writer.rustTemplate("struct $name(pub(crate) $inner);", *codegenScope)
        if (constrainedTypeVisibility == Visibility.PUBCRATE) {
            Attribute.AllowUnused.render(writer)
        }
        writer.rustTemplate(
            """
            impl $name {
                /// ${rustDocsParseMethod(name, inner)}
                pub fn parse(value: $inner) -> Result<Self, #{ConstraintViolation}> {
                    Self::try_from(value)
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
            
            impl #{TryFrom}<$inner> for $name {
                type Error = #{ConstraintViolation};
                
                fn try_from(value: $inner) -> Result<Self, Self::Error> {
                    let length = value.len();
                    if $condition {
                        Ok(Self(value))
                    } else {
                        Err(#{ConstraintViolation}::Length(length))
                    }
                }
            }
            """,
            *codegenScope
        )

        if (unconstrainedSymbol != null) {
            writer.rustTemplate(
                """
                impl #{ConstrainedTrait} for $name  {
                    type Unconstrained = #{UnconstrainedSymbol};
                }
                """,
                "ConstrainedTrait" to RuntimeType.ConstrainedTrait(),
                "UnconstrainedSymbol" to unconstrainedSymbol,
            )
        }

        if (!publicConstrainedTypes) {
            // Note that if public constrained types is not enabled, then the regular `symbolProvider` produces
            // "fully unconstrained" symbols for all shapes (i.e. as if the shapes didn't have any constraint traits).
            writer.rustTemplate(
                """
                impl #{From}<$name> for #{FullyUnconstrainedSymbol} {
                    fn from(constrained: $name) -> Self {
                        constrained.into_inner()
                            .into_iter()
                            .map(|(k, v)| (k.into(), v.into()))
                            .collect()
                    }
                }
                """,
                "From" to RuntimeType.From,
                "FullyUnconstrainedSymbol" to symbolProvider.toSymbol(shape),
            )
        }
    }
}
