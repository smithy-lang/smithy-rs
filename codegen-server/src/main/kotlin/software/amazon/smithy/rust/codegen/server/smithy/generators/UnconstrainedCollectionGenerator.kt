/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Visibility
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.smithy.ConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.PubCrateConstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.UnconstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.smithy.makeMaybeConstrained

// TODO Docs
class UnconstrainedCollectionGenerator(
    val model: Model,
    val symbolProvider: RustSymbolProvider,
    private val unconstrainedShapeSymbolProvider: UnconstrainedShapeSymbolProvider,
    private val pubCrateConstrainedShapeSymbolProvider: PubCrateConstrainedShapeSymbolProvider,
    private val constraintViolationSymbolProvider: ConstraintViolationSymbolProvider,
    private val publicConstrainedTypes: Boolean,
    private val unconstrainedModuleWriter: RustWriter,
    private val modelsModuleWriter: RustWriter,
    val shape: CollectionShape
) {
    private val symbol = unconstrainedShapeSymbolProvider.toSymbol(shape)
    private val name = symbol.name

    fun render() {
        check(shape.canReachConstrainedShape(model, symbolProvider))

        val module = symbol.namespace.split(symbol.namespaceDelimiter).last()
        val innerShape = model.expectShape(shape.member.target)
        val innerUnconstrainedSymbol = unconstrainedShapeSymbolProvider.toSymbol(innerShape)
        val constrainedSymbol = pubCrateConstrainedShapeSymbolProvider.toSymbol(shape)
        val constraintViolationSymbol = constraintViolationSymbolProvider.toSymbol(shape)
        val constraintViolationName = constraintViolationSymbol.name
        val innerConstraintViolationSymbol = constraintViolationSymbolProvider.toSymbol(innerShape)

        unconstrainedModuleWriter.withModule(module, RustMetadata(visibility = Visibility.PUBCRATE)) {
            rustTemplate(
                """
                ##[derive(Debug, Clone)]
                pub(crate) struct $name(pub(crate) std::vec::Vec<#{InnerUnconstrainedSymbol}>);
                
                impl From<$name> for #{MaybeConstrained} {
                    fn from(value: $name) -> Self {
                        Self::Unconstrained(value)
                    }
                }
                
                impl #{TryFrom}<$name> for #{ConstrainedSymbol} {
                    type Error = #{ConstraintViolationSymbol};
                
                    fn try_from(value: $name) -> Result<Self, Self::Error> {
                        let res: Result<_, #{InnerConstraintViolationSymbol}> = value
                            .0
                            .into_iter()
                            .map(|inner| inner.try_into())
                            .collect();
                        res.map(Self)   
                           .map_err(#{ConstraintViolationSymbol})
                    }
                }
                """,
                "InnerUnconstrainedSymbol" to innerUnconstrainedSymbol,
                "InnerConstraintViolationSymbol" to innerConstraintViolationSymbol,
                "ConstrainedSymbol" to constrainedSymbol,
                "ConstraintViolationSymbol" to constraintViolationSymbol,
                "MaybeConstrained" to constrainedSymbol.makeMaybeConstrained(),
                "TryFrom" to RuntimeType.TryFrom,
            )

//            if (!publicConstrainedTypes) {
//                renderFromFullyUnconstrainedForUnconstrained(this)
//            }
        }

        modelsModuleWriter.withModule(
            constraintViolationSymbol.namespace.split(constraintViolationSymbol.namespaceDelimiter).last()
        ) {
            rustTemplate(
                """
                ##[derive(Debug, PartialEq)]
                pub struct $constraintViolationName(pub(crate) #{InnerConstraintViolationSymbol});
                """,
                "InnerConstraintViolationSymbol" to innerConstraintViolationSymbol,
            )
        }
    }

    private fun renderFromFullyUnconstrainedForUnconstrained(writer: RustWriter) {
        // Note that if public constrained types is not enabled, then the regular `symbolProvider` produces
        // "fully unconstrained" symbols for all shapes (i.e. as if the shapes didn't have any constraint traits).
        writer.rustTemplate(
            """
            impl #{From}<#{FullyUnconstrainedSymbol}> for $name {
                fn from(fully_unconstrained: #{FullyUnconstrainedSymbol}) -> Self {
                    Self(fully_unconstrained.into_iter().map(|v| v.into()).collect())
                }
            }
            """,
            "From" to RuntimeType.From,
            "FullyUnconstrainedSymbol" to symbolProvider.toSymbol(shape),
        )
    }
}
