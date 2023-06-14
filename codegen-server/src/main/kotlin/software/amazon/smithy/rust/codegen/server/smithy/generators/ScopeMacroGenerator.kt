/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext

class ScopeMacroGenerator(
    private val codegenContext: ServerCodegenContext,
) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            "SmithyHttpServer" to ServerCargoDependency.smithyHttpServer(runtimeConfig).toType(),
        )

    /** Calculate all `operationShape`s contained within the `ServiceShape`. */
    private val index = TopDownIndex.of(codegenContext.model)
    private val operations = index.getContainedOperations(codegenContext.serviceShape).toSortedSet(compareBy { it.id })

    private fun macro(): Writable = writable {
        val firstOperationName = codegenContext.symbolProvider.toSymbol(operations.first()).name
                val operationNames = operations.joinToString(" ") { codegenContext.symbolProvider.toSymbol(it).name }
        val operationBranches = operations
            .map { codegenContext.symbolProvider.toSymbol(it).name }.joinToString("") {
                """
                // $it match found, pop from both `member` and `not_member`
                (@ $ name: ident, $ predicate: ident ($it $($ member: ident)*) ($($ temp: ident)*) ($it $($ not_member: ident)*)) => {
                    scope! { @ $ name, $ predicate ($($ member)*) ($($ temp)*) ($($ not_member)*) }
                };
                // $it match not found, pop from `not_member` into `temp` stack
                (@ $ name: ident, $ predicate: ident ($it $($ member: ident)*) ($($ temp: ident)*) ($ other: ident $($ not_member: ident)*)) => {
                    scope! { @ $ name, $ predicate ($it $($ member)*) ($ other $($ temp)*) ($($ not_member)*) }
                };
                """
            }.joinToString("")

        rustTemplate(
            """
            /// A macro to help with scoping plugins to a subset of all operations.
            ///
            /// In contrast to [`aws_smithy_http_server::scope`](#{SmithyHttpServer}::scope), this macro has knowledge
            /// of the service and any operations _not_ specified will be placed in the opposing group.
            ///
            /// ## Example
            ///
            /// ```rust
            /// scope! {
            ///     /// Includes [`$firstOperationName`], excluding all other operations.
            ///     struct ScopeA {
            ///         includes: [$firstOperationName],
            ///     }
            /// }
            ///
            /// scope! {
            ///     /// Excludes [`$firstOperationName`], excluding all other operations.
            ///     struct ScopeB {
            ///         excludes: [$firstOperationName]
            ///     }
            /// }
            /// ```
            ##[macro_export]
            macro_rules! scope {
                // Completed, render impls
                (@ $ name: ident, $ predicate: ident () ($($ temp: ident)*) ($($ not_member: ident)*)) => {
                    $(
                        impl #{SmithyHttpServer}::plugin::scoped::Membership<$ temp> for $ name {
                            type Contains = #{SmithyHttpServer}::plugin::scoped::$ predicate;
                        }
                    )*
                    $(
                        impl #{SmithyHttpServer}::plugin::scoped::Membership<$ not_member> for $ name {
                            type Contains = #{SmithyHttpServer}::plugin::scoped::$ predicate;
                        }
                    )*
                };
                // All `not_member`s exhausted, move `temp` into `not_member`
                (@ $ name: ident, $ predicate: ident ($($ member: ident)*) ($($ temp: ident)*) ()) => {
                    scope! { @ $ name, $ predicate ($($ member)*) () ($($ temp)*) }
                };
                $operationBranches
                (
                    $(##[$ attrs:meta])*
                    $ vis:vis struct $ name:ident {
                        includes: [$($ include:ident),*]
                    }
                ) => {
                    use $ crate::operation_shape::*;
                    use #{SmithyHttpServer}::scope as scope_runtime;
                    scope_runtime! {
                        $(##[$ attrs])*
                        $ vis struct $ name {
                            includes: [$($ include),*],
                            excludes: []
                        }
                    }
                    scope! { @ $ name, False ($($ include)*) () ($operationNames) }
                };
                (
                    $(##[$ attrs:meta])*
                    $ vis:vis struct $ name:ident {
                        excludes: [$($ exclude:ident),*]
                    }
                ) => {
                    use $ crate::operation_shape::*;
                    use #{SmithyHttpServer}::scope as scope_runtime;

                    scope_runtime! {
                        $(##[$ attrs])*
                        $ vis struct $ name {
                            includes: [],
                            excludes: [$($ exclude),*]
                        }
                    }
                    scope! { @ $ name, True ($($ exclude)*) () ($operationNames) }
                };
            }
            """,
            *codegenScope,
        )
    }

    fun render(writer: RustWriter) {
        macro()(writer)
    }
}
