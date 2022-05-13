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
import software.amazon.smithy.rust.codegen.smithy.ConstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.UnconstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.smithy.isDirectlyConstrained

// TODO Docs
// TODO Unit tests
class ConstrainedCollectionShape(
    val model: Model,
    val symbolProvider: RustSymbolProvider,
    private val unconstrainedShapeSymbolProvider: UnconstrainedShapeSymbolProvider,
    private val constrainedShapeSymbolProvider: ConstrainedShapeSymbolProvider,
    val writer: RustWriter,
    val shape: CollectionShape
) {
    fun render() {
        check(shape.canReachConstrainedShape(model, symbolProvider))

        val symbol = symbolProvider.toSymbol(shape)
        val constrainedSymbol = constrainedShapeSymbolProvider.toSymbol(shape)
        val unconstrainedSymbol = unconstrainedShapeSymbolProvider.toSymbol(shape)
        val module = constrainedSymbol.namespace.split(constrainedSymbol.namespaceDelimiter).last()
        val name = constrainedSymbol.name
        val innerShape = model.expectShape(shape.member.target)
        val innerConstrainedSymbol = constrainedShapeSymbolProvider.toSymbol(innerShape)

        // If the target member shape is itself _not_ directly constrained, and is an aggregate non-Structure shape,
        // then its corresponding constrained type is the `pub(crate)` wrapper tuple type, which needs converting into
        // the public type the user is exposed to. The two types are isomorphic, and we can convert between them using
        // `From`. So we track this particular case here in order to iterate over the list's members and convert
        // each of them.
        //
        // Note that we could add the iteration code unconditionally and it would still be correct, but the `into()` calls
        // would be useless. Clippy flags this as [`useless_conversion`]. We could deactivate the lint, but it's probably
        // best that we just don't emit a useless iteration, lest the compiler not optimize it away, and to make the
        // generated code a little bit simpler.
        //
        // [`useless_conversion`]: https://rust-lang.github.io/rust-clippy/master/index.html#useless_conversion.
        val targetNeedsConstraining =
            !innerShape.isDirectlyConstrained(symbolProvider) && (innerShape is CollectionShape || innerShape.isMapShape)

        writer.withModule(module, RustMetadata(visibility = Visibility.PUBCRATE)) {
            rustTemplate(
                """
                ##[derive(Debug, Clone)]
                pub(crate) struct $name(pub(crate) std::vec::Vec<#{InnerConstrainedSymbol}>);
                
                impl #{ConstrainedTrait} for $name  {
                    type Unconstrained = #{UnconstrainedSymbol};
                }
                
                impl From<#{Symbol}> for $name {
                    fn from(v: #{Symbol}) -> Self {
                        ${ if (targetNeedsConstraining) {
                            "Self(v.into_iter().map(|item| item.into()).collect())"
                        } else {
                            "Self(v)"
                        } }
                    }
                }

                impl From<$name> for #{Symbol} {
                    fn from(v: $name) -> Self {
                        ${ if (targetNeedsConstraining) {
                            "v.0.into_iter().map(|item| item.into()).collect()"
                        } else {
                            "v.0"
                        } }
                    }
                }
                """,
                "InnerConstrainedSymbol" to innerConstrainedSymbol,
                "ConstrainedTrait" to RuntimeType.ConstrainedTrait(),
                "UnconstrainedSymbol" to unconstrainedSymbol,
                "Symbol" to symbol,
            )
        }
    }
}
