/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.makeOptional
import software.amazon.smithy.rust.codegen.core.smithy.mapRustType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.nestedAccessorName

/** Generator for accessing nested fields through optional values **/
class NestedAccessorGenerator(private val codegenContext: CodegenContext) {
    private val symbolProvider = codegenContext.symbolProvider
    private val module = RustModule.private("lens")

    /**
     * Generate an accessor on [root] that consumes [root] and returns an `Option<T>` for the nested item
     */
    fun generateOwnedAccessor(
        root: StructureShape,
        path: List<MemberShape>,
    ): RuntimeType {
        check(path.isNotEmpty()) { "must not be called on an empty path" }
        val baseType = symbolProvider.toSymbol(path.last())
        val fnName = symbolProvider.nestedAccessorName(codegenContext.serviceShape, "", root, path)
        return RuntimeType.forInlineFun(fnName, module) {
            rustTemplate(
                """
                pub(crate) fn $fnName(input: #{Input}) -> #{Output} {
                    #{body:W}
                }
                """,
                "Input" to symbolProvider.toSymbol(root),
                "Output" to baseType.makeOptional(),
                "body" to generateBody(path, false),
            )
        }
    }

    /**
     * Generate an accessor on [root] that takes a reference and returns an `Option<&T>` for the nested item
     */
    fun generateBorrowingAccessor(
        root: StructureShape,
        path: List<MemberShape>,
    ): RuntimeType {
        check(path.isNotEmpty()) { "must not be called on an empty path" }
        val baseType = symbolProvider.toSymbol(path.last()).makeOptional()
        val fnName = symbolProvider.nestedAccessorName(codegenContext.serviceShape, "ref", root, path)
        val referencedType = baseType.mapRustType { (it as RustType.Option).referenced(lifetime = null) }
        return RuntimeType.forInlineFun(fnName, module) {
            rustTemplate(
                """
                pub(crate) fn $fnName(input: &#{Input}) -> #{Output} {
                    #{body:W}
                }
                """,
                "Input" to symbolProvider.toSymbol(root),
                "Output" to referencedType,
                "body" to generateBody(path, true),
            )
        }
    }

    private fun generateBody(
        path: List<MemberShape>,
        reference: Boolean,
    ): Writable =
        writable {
            val ref =
                if (reference) {
                    "&"
                } else {
                    ""
                }
            if (path.isEmpty()) {
                rustTemplate("#{Some}(input)", *preludeScope)
            } else {
                val head = path.first()
                if (symbolProvider.toSymbol(head).isOptional()) {
                    rustTemplate(
                        """
                        let input = ${ref}input.${symbolProvider.toMemberName(head)}?;
                        """,
                        *preludeScope,
                    )
                } else {
                    rust("let input = input.${symbolProvider.toMemberName(head)};")
                }
                // Note: although _this_ function is recursive, it generates a series of `if let` statements with early returns.
                generateBody(path.drop(1), reference)(this)
            }
        }
}
