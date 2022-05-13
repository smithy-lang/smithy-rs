/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.render
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.smithy.ConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.wrapMaybeConstrained
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

        val symbol = symbolProvider.toSymbol(shape)
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

        // TODO Docs for everything.
        // TODO Display impl.
        // Note that we're using the linear time check `chars().count()` instead of `len()` on the input value, since the
        // Smithy specification says the `length` trait counts the number of Unicode code points when applied to string shapes.
        // https://awslabs.github.io/smithy/1.0/spec/core/constraint-traits.html#length-trait
        writer.rustTemplate(
            """
            ##[derive(Debug, Clone, PartialEq, Eq, Hash)]
            pub struct $name($inner);
            
            impl $name {
                pub fn parse(value: $inner) -> Result<Self, #{ConstraintViolation}> {
                    Self::try_from(value)
                }
            }
            
            impl #{ConstrainedTrait} for $name  {
                type Unconstrained = $inner;
            }
            
            impl From<$inner> for #{MaybeConstrained} {
                fn from(value: $inner) -> Self {
                    Self::Unconstrained(value)
                }
            }
            
            impl std::ops::Deref for $name {
                type Target = String;
            
                fn deref(&self) -> &Self::Target {
                    &self.0
                }
            }
            
            impl AsRef<str> for $name {
                fn as_ref(&self) -> &str {
                    self.0.as_str()
                }
            }
            
            impl std::fmt::Display for $name {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                   write!(f, "{}", self.0)
                }
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
            "ConstraintViolation" to constraintViolation,
            "MaybeConstrained" to symbol.wrapMaybeConstrained(),
        )
    }
}
