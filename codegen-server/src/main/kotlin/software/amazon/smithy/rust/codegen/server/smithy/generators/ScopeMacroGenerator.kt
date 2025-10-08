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
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext

class ScopeMacroGenerator(
    private val codegenContext: ServerCodegenContext,
) {
    /** Calculate all `operationShape`s contained within the `ServiceShape`. */
    private val index = TopDownIndex.of(codegenContext.model)
    private val operations = index.getContainedOperations(codegenContext.serviceShape).toSortedSet(compareBy { it.id })

    private fun macro(): Writable {
        val firstOperation = operations.firstOrNull() ?: return writable {  }
        return writable {
            val firstOperationName = codegenContext.symbolProvider.toSymbol(firstOperation).name.toPascalCase()
            val operationNames =
                operations.joinToString(" ") {
                    codegenContext.symbolProvider.toSymbol(it).name.toPascalCase()
                }

            // When writing `macro_rules!` we add whitespace between `$` and the arguments to avoid Kotlin templating.

            // To achieve the desired API we need to calculate the set theoretic complement `B \ A`.
            // The macro below, for rules prefixed with `@`, encodes a state machine which performs this.
            // The initial state is `(A) () (B)`, where `A` and `B` are lists of elements of `A` and `B`.
            // The rules, in order:
            // - Terminate on pattern `() (t0, t1, ...) (b0, b1, ...)`, the complement has been calculated as
            // `{ t0, t1, ..., b0, b1, ...}`.
            // - Send pattern `(x, a0, a1, ...) (t0, t1, ...) (x, b0, b1, ...)` to
            // `(a0, a1, ...) (t0, t1, ...) (b0, b1, ...)`, eliminating a matching `x` from `A` and `B`.
            // - Send pattern `(a0, a1, ...) (t0, t1, ...) ()` to `(a0, a1, ...) () (t0, t1, ...)`, restarting the search.
            // - Send pattern `(a0, a1, ...) (t0, t1, ...) (b0, b1, ...)` to `(a0, a1, ...) (b0, t0, t1, ...) (b1, ...)`,
            // iterating through the `B`.
            val operationBranches =
                operations
                    .map { codegenContext.symbolProvider.toSymbol(it).name.toPascalCase() }.joinToString("") {
                        """
                            // $it match found, pop from both `member` and `not_member`
                            (@ $ name: ident, $ contains: ident ($it $($ member: ident)*) ($($ temp: ident)*) ($it $($ not_member: ident)*)) => {
                                scope! { @ $ name, $ contains ($($ member)*) ($($ temp)*) ($($ not_member)*) }
                            };
                            // $it match not found, pop from `not_member` into `temp` stack
                            (@ $ name: ident, $ contains: ident ($it $($ member: ident)*) ($($ temp: ident)*) ($ other: ident $($ not_member: ident)*)) => {
                                scope! { @ $ name, $ contains ($it $($ member)*) ($ other $($ temp)*) ($($ not_member)*) }
                            };
                            """
                    }
            val crateName = codegenContext.moduleUseName()

            // If we have a second operation we can perform further checks
            val otherOperationName: String? =
                operations.toList().getOrNull(1)?.let {
                    codegenContext.symbolProvider.toSymbol(it).name
                }
            val furtherTests =
                if (otherOperationName != null) {
                    writable {
                        rustTemplate(
                            """
                                /// ## let a = Plugin::<(), $otherOperationName, u64>::apply(&scoped_a, 6);
                                /// ## let b = Plugin::<(), $otherOperationName, u64>::apply(&scoped_b, 6);
                                /// ## assert_eq!(a, 6_u64);
                                /// ## assert_eq!(b, 3_u32);
                                """,
                        )
                    }
                } else {
                    writable {}
                }

            rustTemplate(
                """
                    /// A macro to help with scoping [plugins](crate::server::plugin) to a subset of all operations.
                    ///
                    /// In contrast to [`crate::server::scope`](crate::server::scope), this macro has knowledge
                    /// of the service and any operations _not_ specified will be placed in the opposing group.
                    ///
                    /// ## Example
                    ///
                    /// ```rust
                    /// scope! {
                    ///     /// Includes [`$firstOperationName`], excluding all other operations.
                    ///     struct ScopeA {
                    ///         includes: [$firstOperationName]
                    ///     }
                    /// }
                    ///
                    /// scope! {
                    ///     /// Excludes [`$firstOperationName`], excluding all other operations.
                    ///     struct ScopeB {
                    ///         excludes: [$firstOperationName]
                    ///     }
                    /// }
                    ///
                    /// ## use $crateName::server::plugin::{Plugin, Scoped};
                    /// ## use $crateName::scope;
                    /// ## struct MockPlugin;
                    /// ## impl<S, Op, T> Plugin<S, Op, T> for MockPlugin { type Output = u32; fn apply(&self, input: T) -> u32 { 3 } }
                    /// ## let scoped_a = Scoped::new::<ScopeA>(MockPlugin);
                    /// ## let scoped_b = Scoped::new::<ScopeB>(MockPlugin);
                    /// ## let a = Plugin::<(), $crateName::operation_shape::$firstOperationName, u64>::apply(&scoped_a, 6);
                    /// ## let b = Plugin::<(), $crateName::operation_shape::$firstOperationName, u64>::apply(&scoped_b, 6);
                    /// ## assert_eq!(a, 3_u32);
                    /// ## assert_eq!(b, 6_u64);
                    /// ```
                    ##[macro_export]
                    macro_rules! scope {
                        // Completed, render impls
                        (@ $ name: ident, $ contains: ident () ($($ temp: ident)*) ($($ not_member: ident)*)) => {
                            $(
                                impl $ crate::server::plugin::scoped::Membership<$ temp> for $ name {
                                    type Contains = $ crate::server::plugin::scoped::$ contains;
                                }
                            )*
                            $(
                                impl $ crate::server::plugin::scoped::Membership<$ not_member> for $ name {
                                    type Contains = $ crate::server::plugin::scoped::$ contains;
                                }
                            )*
                        };
                        // All `not_member`s exhausted, move `temp` into `not_member`
                        (@ $ name: ident, $ contains: ident ($($ member: ident)*) ($($ temp: ident)*) ()) => {
                            scope! { @ $ name, $ contains ($($ member)*) () ($($ temp)*) }
                        };
                        $operationBranches
                        (
                            $(##[$ attrs:meta])*
                            $ vis:vis struct $ name:ident {
                                includes: [$($ include:ident),*]
                            }
                        ) => {
                            use $ crate::operation_shape::*;
                            $ crate::server::scope! {
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
    
                            $ crate::server::scope! {
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
                "FurtherTests" to furtherTests,
            )
        }
    }

    fun render(writer: RustWriter) {
        macro()(writer)
    }
}
