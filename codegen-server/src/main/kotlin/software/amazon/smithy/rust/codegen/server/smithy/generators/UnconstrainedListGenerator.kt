/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Visibility
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.smithy.ConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.ConstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.UnconstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.smithy.wrapMaybeConstrained

// TODO Docs
class UnconstrainedListGenerator(
    val model: Model,
    val symbolProvider: RustSymbolProvider,
    private val unconstrainedShapeSymbolProvider: UnconstrainedShapeSymbolProvider,
    private val constrainedShapeSymbolProvider: ConstrainedShapeSymbolProvider,
    private val constraintViolationSymbolProvider: ConstraintViolationSymbolProvider,
    val writer: RustWriter,
    val shape: ListShape
) {
    fun render() {
        check(shape.canReachConstrainedShape(model, symbolProvider))

        val symbol = unconstrainedShapeSymbolProvider.toSymbol(shape)
        val module = symbol.namespace.split(symbol.namespaceDelimiter).last()
        val name = symbol.name
        val innerShape = model.expectShape(shape.member.target)
        val innerUnconstrainedSymbol = unconstrainedShapeSymbolProvider.toSymbol(innerShape)
        val constrainedSymbol = constrainedShapeSymbolProvider.toSymbol(shape)
        val constraintViolationName = constraintViolationSymbolProvider.toSymbol(shape).name
        val innerConstraintViolationSymbol = constraintViolationSymbolProvider.toSymbol(innerShape)

        // TODO Move implementation of ConstrainedTrait to the constrained module.
        // TODO Don't be lazy and don't use `Result<_, >`.
        writer.withModule(module, RustMetadata(visibility = Visibility.PUBCRATE)) {
            rustTemplate(
                """
                ##[derive(Debug, Clone)]
                pub(crate) struct $name(pub(crate) Vec<#{InnerUnconstrainedSymbol}>);
                
                impl #{ConstrainedTrait} for #{ConstrainedSymbol}  {
                    type Unconstrained = $name;
                }
                
                impl From<$name> for #{MaybeConstrained} {
                    fn from(value: $name) -> Self {
                        Self::Unconstrained(value)
                    }
                }
                
                ##[derive(Debug, PartialEq)]
                pub struct $constraintViolationName(pub(crate) #{InnerConstraintViolationSymbol});
                
                impl std::convert::TryFrom<$name> for #{ConstrainedSymbol} {
                    type Error = $constraintViolationName;
                
                    fn try_from(value: $name) -> Result<Self, Self::Error> {
                        let res: Result<_, #{InnerConstraintViolationSymbol}> = value
                            .0
                            .into_iter()
                            .map(|inner| {
                                use std::convert::TryInto;
                                inner.try_into()
                            })
                            .collect();
                        res.map(|inner| Self(inner))   
                           .map_err(|err| ConstraintViolation(err))
                    }
                }
                """,
                "InnerUnconstrainedSymbol" to innerUnconstrainedSymbol,
                "InnerConstraintViolationSymbol" to innerConstraintViolationSymbol,
                "ConstrainedSymbol" to constrainedSymbol,
                "MaybeConstrained" to constrainedSymbol.wrapMaybeConstrained(),
                "ConstrainedTrait" to RuntimeType.ConstrainedTrait(),
            )
        }
    }
}
