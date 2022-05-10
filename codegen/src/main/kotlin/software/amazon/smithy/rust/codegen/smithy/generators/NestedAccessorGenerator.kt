/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.Visibility
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.makeOptional
import software.amazon.smithy.rust.codegen.smithy.mapRustType
import software.amazon.smithy.rust.codegen.smithy.protocols.lensName

/** Generator for accessing nested fields through optional values **/
class NestedAccessorGenerator(private val symbolProvider: RustSymbolProvider) {
    private val module = RustModule("lens", RustMetadata(visibility = Visibility.PUBLIC), "Generated accessors for nested fields")
    /**
     * Generate an accessor on [root] that consumes [root] and returns an `Option<T>` for the nested item
     */
    fun generateOwnedAccessor(root: StructureShape, path: List<MemberShape>): RuntimeType {
        check(path.isNotEmpty()) { "must not be called on an empty path" }
        val baseType = symbolProvider.toSymbol(path.last())
        val fnName = symbolProvider.lensName("", root, path)
        return RuntimeType.forInlineFun(fnName, module) {
            it.rustTemplate(
                """
                pub(crate) fn $fnName(input: #{Input}) -> #{Output} {
                    #{body:W}
                }
                """,
                "Input" to symbolProvider.toSymbol(root), "Output" to baseType.makeOptional(), "body" to generateBody(path, false)
            )
        }
    }

    /**
     * Generate an accessor on [root] that takes a reference and returns an `Option<&T>` for the nested item
     */
    fun generateBorrowingAccessor(root: StructureShape, path: List<MemberShape>): RuntimeType {
        check(path.isNotEmpty()) { "must not be called on an empty path" }
        val baseType = symbolProvider.toSymbol(path.last()).makeOptional()
        val fnName = symbolProvider.lensName("ref", root, path)
        val referencedType = baseType.mapRustType { (it as RustType.Option).referenced(lifetime = null) }
        return RuntimeType.forInlineFun(fnName, module) {
            it.rustTemplate(
                """
                pub(crate) fn $fnName(input: &#{Input}) -> #{Output} {
                    #{body:W}
                }
                """,
                "Input" to symbolProvider.toSymbol(root), "Output" to referencedType, "body" to generateBody(path, true)
            )
        }
    }

    private fun generateBody(path: List<MemberShape>, reference: Boolean): Writable = writable {
        val ref = if (reference) { "&" } else { "" }
        if (path.isEmpty()) {
            rust("Some(input)")
        } else {
            val head = path.first()
            if (symbolProvider.toSymbol(head).isOptional()) {
                rust(
                    """
                    let input = match ${ref}input.${symbolProvider.toMemberName(head)} {
                        None => return None,
                        Some(t) => t
                    };
                    """
                )
            } else {
                rust("let input = input.${symbolProvider.toMemberName(head)};")
            }
            // Note: although _this_ function is recursive, it generates a series of `if let` statements with early returns.
            generateBody(path.drop(1), reference)(this)
        }
    }
}
