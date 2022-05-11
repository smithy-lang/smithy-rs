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
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Visibility
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.smithy.ConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.ConstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.UnconstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.smithy.isConstrained
import software.amazon.smithy.rust.codegen.smithy.wrapMaybeConstrained
import software.amazon.smithy.rust.codegen.util.hasTrait

// TODO Docs
class UnconstrainedMapGenerator(
    val model: Model,
    val symbolProvider: RustSymbolProvider,
    private val unconstrainedShapeSymbolProvider: UnconstrainedShapeSymbolProvider,
    constrainedShapeSymbolProvider: ConstrainedShapeSymbolProvider,
    private val constraintViolationSymbolProvider: ConstraintViolationSymbolProvider,
    val writer: RustWriter,
    val shape: MapShape
) {
    private val symbol = unconstrainedShapeSymbolProvider.toSymbol(shape)
    private val name = symbol.name
    private val constraintViolationName = constraintViolationSymbolProvider.toSymbol(shape).name
    private val keyShape = model.expectShape(shape.key.target, StringShape::class.java)
    private val valueShape = model.expectShape(shape.value.target)
    private val constrainedSymbol = if (shape.isConstrained(symbolProvider)) {
        symbolProvider.toSymbol(shape)
    } else {
        constrainedShapeSymbolProvider.toSymbol(shape)
    }

    fun render() {
        check(shape.canReachConstrainedShape(model, symbolProvider))

        val module = symbol.namespace.split(symbol.namespaceDelimiter).last()
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

        // TODO Docs for ConstraintViolation.
        // TODO KeyConstraintViolation and ValueConstraintViolation need to be `#[doc(hidden)]`.
        writer.withModule(module, RustMetadata(visibility = Visibility.PUBCRATE)) {
            rustTemplate(
                """
                ##[derive(Debug, Clone)]
                pub(crate) struct $name(pub(crate) std::collections::HashMap<#{KeySymbol}, #{ValueSymbol}>);
                
                impl From<$name> for #{MaybeConstrained} {
                    fn from(value: $name) -> Self {
                        Self::Unconstrained(value)
                    }
                }
                
                ##[derive(Debug, PartialEq)]
                pub enum $constraintViolationName {
                    ${ if (shape.hasTrait<LengthTrait>()) "Length(usize)," else "" }
                    ${ if (isKeyConstrained(keyShape)) "Key(#{KeyConstraintViolationSymbol})," else "" }
                    ${ if (isValueConstrained(valueShape)) "Value(#{ValueConstraintViolationSymbol})," else "" }
                }
                """,
                "KeySymbol" to keySymbol,
                "ValueSymbol" to valueSymbol,
                *constraintViolationCodegenScope,
                "MaybeConstrained" to constrainedSymbol.wrapMaybeConstrained(),
            )

            renderTryFromUnconstrainedForConstrained(this)
        }
    }

    private fun renderTryFromUnconstrainedForConstrained(writer: RustWriter) {
        writer.rustBlock("impl std::convert::TryFrom<$name> for #{T}", constrainedSymbol) {
            rust("type Error = $constraintViolationName;")

            rustBlock("fn try_from(value: $name) -> Result<Self, Self::Error>") {
                if (listOf(keyShape, valueShape).any { isValueConstrained(it) }) {
                    rustTemplate(
                        """
                        let res: Result<std::collections::HashMap<#{ConstrainedKeySymbol}, #{ConstrainedValueSymbol}>, Self::Error> = value.0
                            .into_iter()
                            .map(|(k, v)| {
                                use std::convert::TryInto;
                                ${if (isKeyConstrained(keyShape)) "let k = k.try_into().map_err(|err| Self::Error::Key(err))?;" else ""}
                                ${if (isValueConstrained(valueShape)) "let v = v.try_into().map_err(|err| Self::Error::Value(err))?;" else ""}
                                Ok((k, v))
                            })
                            .collect();
                        let hm = res?;
                        """,
                        "ConstrainedKeySymbol" to symbolProvider.toSymbol(keyShape),
                        "ConstrainedValueSymbol" to symbolProvider.toSymbol(valueShape),
                    )
                } else {
                    rust("let hm = value.0;")
                }

                if (shape.isConstrained(symbolProvider)) {
                    rust("Self::try_from(hm)")
                } else {
                    rust("Self(hm)")
                }
            }
        }
    }

    private fun isKeyConstrained(shape: StringShape) = shape.isConstrained(symbolProvider)

    private fun isValueConstrained(shape: Shape): Boolean = when (shape) {
        is StructureShape -> shape.canReachConstrainedShape(model, symbolProvider)
        is CollectionShape -> shape.canReachConstrainedShape(model, symbolProvider)
        is MapShape -> shape.canReachConstrainedShape(model, symbolProvider)
        is StringShape -> shape.isConstrained(symbolProvider)
        // TODO(https://github.com/awslabs/smithy-rs/pull/1199) Other constraint traits on simple shapes.
        else -> false
    }
}
