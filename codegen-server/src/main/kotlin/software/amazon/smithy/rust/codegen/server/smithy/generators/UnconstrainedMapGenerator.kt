/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Visibility
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.smithy.ConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.UnconstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.smithy.hasConstraintTrait
import software.amazon.smithy.rust.codegen.smithy.wrapMaybeConstrained

// TODO Docs
class UnconstrainedMapGenerator(
    val model: Model,
    val symbolProvider: RustSymbolProvider,
    private val unconstrainedShapeSymbolProvider: UnconstrainedShapeSymbolProvider,
    private val constraintViolationSymbolProvider: ConstraintViolationSymbolProvider,
    val writer: RustWriter,
    val shape: MapShape
) {
    fun render() {
        check(shape.canReachConstrainedShape(model, symbolProvider))

        val symbol = unconstrainedShapeSymbolProvider.toSymbol(shape)
        val module = symbol.namespace.split(symbol.namespaceDelimiter).last()
        val name = symbol.name
        val keyShape = model.expectShape(shape.key.target, StringShape::class.java)
        val valueShape = model.expectShape(shape.value.target)
        val keySymbol = if (isKeyConstrained(keyShape)) {
            unconstrainedShapeSymbolProvider.toSymbol(keyShape)
        } else {
            symbolProvider.toSymbol(keyShape)
        }
        val valueSymbol = if (isValueConstrained(valueShape)) {
            unconstrainedShapeSymbolProvider.toSymbol(valueShape)
        } else {
            symbolProvider.toSymbol(valueShape)
        }
        // TODO(https://github.com/awslabs/smithy-rs/pull/1199): We will need a `ConstrainedSymbolProvider` when we have constraint traits.
        val constrainedSymbol = symbolProvider.toSymbol(shape)
        val constraintViolationName = constraintViolationSymbolProvider.toSymbol(shape).name
        val constraintViolationCodegenScope = listOfNotNull(
            if (isKeyConstrained(keyShape)) {
                "KeyConstraintViolationSymbol" to constraintViolationSymbolProvider.toSymbol(keyShape)
            } else {
                null
            },
            if (isValueConstrained(valueShape)) {
                "ValueConstraintViolationSymbol" to constraintViolationSymbolProvider.toSymbol(valueShape)
            } else {
                null
            },
        ).toTypedArray()

        // TODO The implementation of the `Constrained` trait is probably not for the correct type. There might be more than
        //    one "path" to an e.g. HashMap<HashMap<StructA>> with different constraint traits along the path, because constraint
        //    traits can be applied to members, or simply because the model might have two different maps holding `StructA`.
        //    So we will have to newtype things.
        writer.withModule(module, RustMetadata(visibility = Visibility.PUBCRATE)) {
            rustTemplate(
                """
                ##[derive(Debug, Clone)]
                pub(crate) struct $name(pub(crate) std::collections::HashMap<#{KeySymbol}, #{ValueSymbol}>);
                
                impl #{ConstrainedTrait} for #{ConstrainedSymbol}  {
                    type Unconstrained = $name;
                }
                
                impl From<$name> for #{MaybeConstrained} {
                    fn from(value: $name) -> Self {
                        Self::Unconstrained(value)
                    }
                }
                
                ##[derive(Debug, PartialEq)]
                pub enum $constraintViolationName {
                    ${ if (isKeyConstrained(keyShape)) "Key(#{KeyConstraintViolationSymbol})," else "" }
                    ${ if (isValueConstrained(valueShape)) "Value(#{ValueConstraintViolationSymbol})," else "" }
                }
                
                impl std::convert::TryFrom<$name> for #{ConstrainedSymbol} {
                    type Error = $constraintViolationName;
                
                    fn try_from(value: $name) -> Result<Self, Self::Error> {
                        value
                            .0
                            .into_iter()
                            .map(|(k, v)| {
                                use std::convert::TryInto;
                                ${ if (isKeyConstrained(keyShape)) "let k = k.try_into().map_err(|err| Self::Error::Key(err))?;" else "" }
                                ${ if (isValueConstrained(valueShape)) "let v = v.try_into().map_err(|err| Self::Error::Value(err))?;" else "" }
                                Ok((k, v))
                            })
                            .collect()
                    }
                }
                """,
                "KeySymbol" to keySymbol,
                "ValueSymbol" to valueSymbol,
                *constraintViolationCodegenScope,
                "ConstrainedSymbol" to constrainedSymbol,
                "MaybeConstrained" to constrainedSymbol.wrapMaybeConstrained(),
                "ConstrainedTrait" to RuntimeType.ConstrainedTrait(),
            )
        }
    }

    private fun isKeyConstrained(shape: StringShape) = shape.hasConstraintTrait()

    private fun isValueConstrained(shape: Shape): Boolean = when (shape) {
        is StructureShape -> shape.canReachConstrainedShape(model, symbolProvider)
        is CollectionShape -> shape.canReachConstrainedShape(model, symbolProvider)
        is MapShape -> shape.canReachConstrainedShape(model, symbolProvider)
        // TODO(https://github.com/awslabs/smithy-rs/pull/1199) Constraint traits on simple shapes.
        else -> false
    }
}
