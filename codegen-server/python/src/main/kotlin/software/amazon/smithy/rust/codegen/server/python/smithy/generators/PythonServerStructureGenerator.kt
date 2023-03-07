/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.asDeref
import software.amazon.smithy.rust.codegen.core.rustlang.asRef
import software.amazon.smithy.rust.codegen.core.rustlang.deprecatedShape
import software.amazon.smithy.rust.codegen.core.rustlang.isCopy
import software.amazon.smithy.rust.codegen.core.rustlang.isDeref
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustInlineTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonType
import software.amazon.smithy.rust.codegen.server.python.smithy.pythonType
import software.amazon.smithy.rust.codegen.server.python.smithy.renderAsDocstring

/**
 * To share structures defined in Rust with Python, `pyo3` provides the `PyClass` trait.
 * This class generates input / output / error structures definitions and implements the
 * `PyClass` trait.
 */
class PythonServerStructureGenerator(
    model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val writer: RustWriter,
    private val shape: StructureShape,
) : StructureGenerator(model, symbolProvider, writer, shape, emptyList()) {

    private val pyO3 = PythonServerCargoDependency.PyO3.toType()

    override fun renderStructure() {
        if (shape.hasTrait<ErrorTrait>()) {
            Attribute(
                writable {
                    rustInlineTemplate(
                        "#{pyclass}(extends = #{PyException})",
                        "pyclass" to pyO3.resolve("pyclass"),
                        "PyException" to pyO3.resolve("exceptions::PyException"),
                    )
                },
            ).render(writer)
        } else {
            Attribute(pyO3.resolve("pyclass")).render(writer)
        }
        writer.rustTemplate("#{ConstructorSignature:W}", "ConstructorSignature" to renderConstructorSignature())
        super.renderStructure()
        renderPyO3Methods()
    }

    override fun renderStructureMember(
        writer: RustWriter,
        member: MemberShape,
        memberName: String,
        memberSymbol: Symbol,
    ) {
        writer.addDependency(PythonServerCargoDependency.PyO3)
        // Above, we manually add dependency since we can't use a `RuntimeType` below
        Attribute("pyo3(get, set)").render(writer)
        writer.rustTemplate("#{Signature:W}", "Signature" to renderSymbolSignature(memberSymbol))
        super.renderStructureMember(writer, member, memberName, memberSymbol)
    }

    override fun renderMemberImpl(writer: RustWriter, member: MemberShape, memberName: String, memberSymbol: Symbol) {
        writer.renderMemberDoc(member, memberSymbol)
        writer.deprecatedShape(member)
        var memberType = memberSymbol.rustType()
        var returnType = when {
            memberType.isCopy() -> memberType
            memberType is RustType.Option && memberType.member.isDeref() -> memberType.asDeref()
            memberType.isDeref() -> memberType.asDeref().asRef()
            else -> memberType.asRef()
        }

        writer.rustBlock("pub fn $memberName(&self) -> ${returnType.render()}") {
            when {
                memberType.isCopy() -> rust("self.$memberName")
                memberType is RustType.Option && memberType.member.isDeref() -> rust("self.$memberName.as_deref()")
                memberType is RustType.Option -> rust("self.$memberName.as_ref()")
                memberType.isDeref() -> rust("use std::ops::Deref; self.$memberName.deref()")
                else -> rust("&self.$memberName")
            }
        }
    }

    private fun renderPyO3Methods() {
        Attribute.AllowClippyNewWithoutDefault.render(writer)
        Attribute(pyO3.resolve("pymethods")).render(writer)
        writer.rustTemplate(
            """
            impl $name {
                ##[new]
                pub fn new(#{BodySignature:W}) -> Self {
                    Self {
                        #{BodyMembers:W}
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
            "BodySignature" to renderStructSignatureMembers(),
            "BodyMembers" to renderStructBodyMembers(),
        )
    }

    private fun renderStructSignatureMembers(): Writable =
        writable {
            forEachMember(members) { member, memberName, memberSymbol ->
                val rustType = memberSymbol.rustType()
                val targetShape = model.expectShape(member.target)
                val targetType = when (targetShape) {
                    is UnionShape -> {
                        if (rustType.name.startsWith("PyUnionMarker")) {
                            memberSymbol.rustType()
                        } else {
                            RustType.Opaque(name = "PyUnionMarker${rustType.render(fullyQualified = false)}", namespace = rustType.namespace)
                        }
                    }
                    else -> {
                        memberSymbol.rustType()
                    }
                }
                rust("$memberName: ${targetType.render()},")
            }
        }

    private fun renderStructBodyMembers(): Writable =
        writable {
            forEachMember(members) { member, memberName, memberSymbol ->
                val rustType = memberSymbol.rustType()
                val targetShape = model.expectShape(member.target)
                when (targetShape) {
                    is UnionShape -> {
                        if (rustType.name.startsWith("PyUnionMarker")) {
                            rust("$memberName,")
                        } else {
                            rust("$memberName: $memberName.0,")
                        }
                    }
                    else -> {
                        rust("$memberName,")
                    }
                }
            }
        }

    private fun renderConstructorSignature(): Writable =
        writable {
            forEachMember(members) { _, memberName, memberSymbol ->
                val memberType = memberSymbol.rustType().pythonType()
                rust("/// :param $memberName ${memberType.renderAsDocstring()}:")
            }

            rust("/// :rtype ${PythonType.None.renderAsDocstring()}:")
        }

    private fun renderSymbolSignature(symbol: Symbol): Writable =
        writable {
            val pythonType = symbol.rustType().pythonType()
            rust("/// :type ${pythonType.renderAsDocstring()}:")
        }
}
