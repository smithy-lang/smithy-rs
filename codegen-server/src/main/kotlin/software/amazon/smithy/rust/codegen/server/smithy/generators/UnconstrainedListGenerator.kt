/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.smithy.ConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.UnconstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.smithy.wrapMaybeConstrained

// TODO Docs
// TODO Can we reuse this generator for sets?
class UnconstrainedListGenerator(
    val model: Model,
    val symbolProvider: RustSymbolProvider,
    private val unconstrainedShapeSymbolProvider: UnconstrainedShapeSymbolProvider,
    private val constraintViolationSymbolProvider: ConstraintViolationSymbolProvider,
    val writer: RustWriter,
    val shape: ListShape
) {
    fun render() {
        check(shape.canReachConstrainedShape(model, symbolProvider))

        // TODO Unit test that this is pub(crate).

        // TODO Some of these can become private properties.
        val symbol = unconstrainedShapeSymbolProvider.toSymbol(shape)
        val module = symbol.namespace.split(symbol.namespaceDelimiter).last()
        val name = symbol.name
        val innerShape = model.expectShape(shape.member.target)
        val innerSymbol = unconstrainedShapeSymbolProvider.toSymbol(innerShape)
        // TODO: We will need a `ConstrainedSymbolProvider` when we have constraint traits.
        val constrainedSymbol = symbolProvider.toSymbol(shape)
        val constraintViolationName = constraintViolationSymbolProvider.toSymbol(shape).name
        val innerConstraintViolationSymbol = constraintViolationSymbolProvider.toSymbol(innerShape)

        // TODO Strictly, `ConstrainedTrait` only needs to be implemented if this list is a struct member.
        // TODO The implementation of the Constrained trait is probably not for the correct type. There might be more than
        //    one "path" to an e.g. Vec<Vec<StructA>> with different constraint traits along the path, because constraint
        //    traits can be applied to members, or simply because the model might have two different lists holding `StructA`.
        //    So we might have to newtype things.
        writer.withModule(module, RustMetadata(public = false, pubCrate = true)) {
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
                        let res: Result<Self, #{InnerConstraintViolationSymbol}> = value
                            .0
                            .into_iter()
                            .map(|inner| {
                                use std::convert::TryInto;
                                inner.try_into()
                            })
                            .collect();
                        res.map_err(|err| ConstraintViolation(err))
                    }
                }
                """,
                "InnerUnconstrainedSymbol" to innerSymbol,
                "InnerConstraintViolationSymbol" to innerConstraintViolationSymbol,
                "ConstrainedSymbol" to constrainedSymbol,
                "MaybeConstrained" to constrainedSymbol.wrapMaybeConstrained(),
                "ConstrainedTrait" to RuntimeType.ConstrainedTrait(),
            )
        }
    }
}
