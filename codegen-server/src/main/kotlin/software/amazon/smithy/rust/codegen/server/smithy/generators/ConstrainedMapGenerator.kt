/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.smithy.ConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.UnconstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.util.expectTrait

// TODO Unit tests
/**
 * [ConstrainedMapGenerator] generates a wrapper tuple newtype holding a constrained `std::collections::HashMap`.
 * This type can be built from unconstrained values, yielding a `ConstraintViolation` when the input does not satisfy
 * the constraints.
 *
 * The [`length` trait] is the only constraint trait applicable to map shapes.
 *
 * [`length` trait]: https://awslabs.github.io/smithy/1.0/spec/core/constraint-traits.html#length-trait
 */
class ConstrainedMapGenerator(
    val model: Model,
    val symbolProvider: RustSymbolProvider,
    private val unconstrainedShapeSymbolProvider: UnconstrainedShapeSymbolProvider,
    private val constraintViolationSymbolProvider: ConstraintViolationSymbolProvider,
    val writer: RustWriter,
    val shape: MapShape
) {
    fun render() {
        // The `length` trait is the only constraint trait applicable to map shapes.
        val lengthTrait = shape.expectTrait<LengthTrait>()

        val name = symbolProvider.toSymbol(shape).name
        val inner = "std::collections::HashMap<#{KeySymbol}, #{ValueSymbol}>"
        // TODO This won't work if the map is only used in operation output, because we don't render the constraint violation symbol.
        val constraintViolation = constraintViolationSymbolProvider.toSymbol(shape)

        val condition = if (lengthTrait.min.isPresent && lengthTrait.max.isPresent) {
            "(${lengthTrait.min.get()}..=${lengthTrait.max.get()}).contains(&length)"
        } else if (lengthTrait.min.isPresent) {
            "${lengthTrait.min.get()} <= length"
        } else {
            "length <= ${lengthTrait.max.get()}"
        }

        writer.documentShape(shape, model, note = rustDocsNote(name))
        writer.rustTemplate(
            """
            ##[derive(Debug, Clone, PartialEq)]
            pub struct $name(pub(crate) $inner);
            
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
            
            impl #{ConstrainedTrait} for $name  {
                type Unconstrained = #{UnconstrainedSymbol};
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
            "KeySymbol" to symbolProvider.toSymbol(model.expectShape(shape.key.target)),
            "ValueSymbol" to symbolProvider.toSymbol(model.expectShape(shape.value.target)),
            "ConstrainedTrait" to RuntimeType.ConstrainedTrait(),
            "UnconstrainedSymbol" to unconstrainedShapeSymbolProvider.toSymbol(shape),
            "TryFrom" to RuntimeType.TryFrom,
            "ConstraintViolation" to constraintViolation
        )
    }
}
