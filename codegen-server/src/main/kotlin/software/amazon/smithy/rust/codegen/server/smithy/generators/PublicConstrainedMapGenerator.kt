/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.smithy.ConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.UnconstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.util.expectTrait

// TODO Docs
// TODO Unit tests
class PublicConstrainedMapGenerator(
    val model: Model,
    val symbolProvider: RustSymbolProvider,
    private val unconstrainedShapeSymbolProvider: UnconstrainedShapeSymbolProvider,
    private val constraintViolationSymbolProvider: ConstraintViolationSymbolProvider,
    val writer: RustWriter,
    val shape: MapShape
) {
    fun render() {
        val lengthTrait = shape.expectTrait<LengthTrait>()

        val name = symbolProvider.toSymbol(shape).name
        val inner = "std::collections::HashMap<#{KeySymbol}, #{ValueSymbol}>"
        val constraintViolation = constraintViolationSymbolProvider.toSymbol(shape)

        val condition = if (lengthTrait.min.isPresent && lengthTrait.max.isPresent) {
            "(${lengthTrait.min.get()}..=${lengthTrait.max.get()}).contains(&length)"
        } else if (lengthTrait.min.isPresent) {
            "${lengthTrait.min.get()} <= length"
        } else {
            "length <= ${lengthTrait.max.get()}"
        }

        // TODO Docs for everything.
        writer.rustTemplate(
            """
            ##[derive(Debug, Clone, PartialEq)]
            pub struct $name($inner);
            
            impl $name {
                pub fn parse(value: $inner) -> Result<Self, #{ConstraintViolation}> {
                    Self::try_from(value)
                }
            }
            
            impl #{ConstrainedTrait} for $name  {
                type Unconstrained = #{UnconstrainedSymbol};
            }
            
            impl std::convert::TryFrom<$inner> for $name {
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
            "ConstraintViolation" to constraintViolation
        )
    }
}
