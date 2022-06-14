/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.render
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.util.hasTrait

/**
 * To share structures defined in Rust with Python, `pyo3` provides the `PyClass` trait.
 * This class generates input / output / error structures definitions and implements the
 * `PyClass` trait.
 */
open class PythonServerStructureGenerator(
    model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val writer: RustWriter,
    private val shape: StructureShape
) : StructureGenerator(model, symbolProvider, writer, shape) {

    override fun renderStructure() {
        val symbol = symbolProvider.toSymbol(shape)
        val containerMeta = symbol.expectRustMetadata()
        if (shape.hasTrait<ErrorTrait>()) {
            writer.renderPyClassException()
        } else {
            writer.renderPyClass()
        }
        writer.documentShape(shape, model)
        val withoutDebug = containerMeta.derives.copy(
            derives = containerMeta.derives.derives - RuntimeType.Debug + RuntimeType.Clone
        )
        containerMeta.copy(derives = withoutDebug).render(writer)

        writer.rustBlock("struct $name ${lifetimeDeclaration()}") {
            forEachMember(members) { member, memberName, memberSymbol ->
                renderMemberDoc(member, memberSymbol)
                renderPyGetterSetter()
                memberSymbol.expectRustMetadata().render(this)
                write("$memberName: #T,", symbolProvider.toSymbol(member))
            }
        }
        renderStructureImpl()
        renderDebugImpl()
        renderPyO3Methods()
    }

    private fun renderPyO3Methods() {
        if (shape.hasTrait<ErrorTrait>() || accessorMembers.isNotEmpty()) {
            writer.renderPyMethods()
            writer.rustTemplate(
                """
                impl $name {
                    ##[new]
                    pub fn new(#{bodysignature:W}) -> Self {
                        Self {
                            #{bodymembers:W}
                        }
                    }
                    fn __repr__(&self) -> String  {
                        format!("{self:?}")
                    }
                    fn __str__(&self) -> String {
                        format!("{self:?}")
                    }
                }
                """,
                "bodysignature" to renderStructSignatureMembers(),
                "bodymembers" to renderStructBodyMembers()
            )
        }
    }

    private fun renderStructSignatureMembers(): Writable =
        writable {
            forEachMember(members) { _, memberName, memberSymbol ->
                val memberType = memberSymbol.rustType()
                rust("$memberName: ${memberType.render()},")
            }
        }

    private fun renderStructBodyMembers(): Writable =
        writable {
            forEachMember(members) { _, memberName, _ -> rust("$memberName,") }
        }
}
