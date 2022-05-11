/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.smithy.ConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.util.expectTrait
import software.amazon.smithy.rust.codegen.util.toSnakeCase

// TODO Docs
// TODO Unit tests
class PublicConstrainedStringGenerator(
    val model: Model,
    val symbolProvider: RustSymbolProvider,
    private val constraintViolationSymbolProvider: ConstraintViolationSymbolProvider,
    val writer: RustWriter,
    val shape: StringShape
) {
    fun render() {
        val lengthTrait = shape.expectTrait<LengthTrait>()

        val name = symbolProvider.toSymbol(shape).name
        val inner = "String"
        val constraintViolation = constraintViolationSymbolProvider.toSymbol(shape)

        val minCondition = lengthTrait.min.map { "$it <= length" }
        val maxCondition = lengthTrait.max.map { "length <= $it" }
        val condition = if (minCondition.isPresent && maxCondition.isPresent) {
            "${minCondition.get()} && ${maxCondition.get()}"
        } else if (minCondition.isPresent) {
            minCondition.get()
        } else {
            maxCondition.get()
        }

        // TODO Docs for everything.
        // Note that we're using the linear time check `chars().count()` instead of `len()` on the input value, since the
        // Smithy specification says the `length` trait counts the number of Unicode code points when applied to string shapes.
        // https://awslabs.github.io/smithy/1.0/spec/core/constraint-traits.html#length-trait
        writer.rustTemplate(
            """
            ##[derive(Debug, Clone, PartialEq)]
            pub struct $name($inner);
            
            impl $name {
                pub fn parse(value: $inner) -> Result<Self, #{ConstraintViolation}> {
                    use std::convert::TryFrom;
                    Self::try_from(value)
                }
            }
            
            impl #{ConstrainedTrait} for $name  {
                type Unconstrained = $inner;
            }
            
            pub mod ${name.toSnakeCase()} {
                ##[derive(Debug, PartialEq)]
                pub enum ConstraintViolation {
                    Length(usize),
                }
            }
            
            impl std::convert::TryFrom<$inner> for $name {
                type Error = #{ConstraintViolation};
                
                fn try_from(value: $inner) -> Result<Self, Self::Error> {
                    let length = value.chars().count();
                    if $condition {
                        Ok(Self(value))
                    } else {
                        Err(#{ConstraintViolation}::Length(length))
                    }
                }
            }
            """,
            "ConstrainedTrait" to RuntimeType.ConstrainedTrait(),
            "ConstraintViolation" to constraintViolation
        )
    }
}
