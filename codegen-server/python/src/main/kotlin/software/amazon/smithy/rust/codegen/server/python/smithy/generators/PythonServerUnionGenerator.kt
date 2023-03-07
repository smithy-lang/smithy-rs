/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.isTargetUnit
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency
import software.amazon.smithy.rust.codegen.server.python.smithy.pythonType
import software.amazon.smithy.rust.codegen.server.python.smithy.renderAsDocstring

private fun RustType.renderPy() {
}

class PythonServerUnionGenerator(
    model: Model,
    private val symbolProvider: SymbolProvider,
    private val writer: RustWriter,
    shape: UnionShape,
    private val renderUnknownVariant: Boolean = true,
) : UnionGenerator(model, symbolProvider, writer, shape, renderUnknownVariant) {
    private val sortedMembers: List<MemberShape> = shape.allMembers.values.sortedBy { symbolProvider.toMemberName(it) }
    private val unionSymbol = symbolProvider.toSymbol(shape)

    private val pyo3 = PythonServerCargoDependency.PyO3.toType()

    override fun render() {
        super.render()
        renderPyUnion()
        renderPyUnionDeref()
        renderPyUnionFrom()
    }

    private fun renderPyUnion() {
        writer.rust("""##[pyo3::pyclass(name = "${unionSymbol.name}")]""")
        val containerMeta = unionSymbol.expectRustMetadata()
        containerMeta.render(writer)
        writer.rust("struct PyUnionMarker${unionSymbol.name}(pub ${unionSymbol.name});")
        writer.rustBlockTemplate("impl #{pyo3}::IntoPy<#{pyo3}::PyObject> for ${unionSymbol.name}", "pyo3" to pyo3) {
            rustBlockTemplate("fn into_py(self, py: #{pyo3}::Python<'_>) -> #{pyo3}::PyObject", "pyo3" to pyo3) {
                rustBlock("match self") {
                    sortedMembers.forEach { member ->
                        val variantName = symbolProvider.toMemberName(member)
                        rust("${unionSymbol.name}::$variantName(variant) => variant.into_py(py),")
                    }
                }
            }
        }
        Attribute(pyo3.resolve("pymethods")).render(writer)
        writer.rustBlock("impl PyUnionMarker${unionSymbol.name}") {
            sortedMembers.forEach { member ->
                val funcNamePart = member.memberName.toSnakeCase()
                val variantName = symbolProvider.toMemberName(member)

                if (sortedMembers.size == 1) {
                    Attribute.AllowIrrefutableLetPatterns.render(this)
                }
                renderNewVariant(writer, model, symbolProvider, member, variantName, funcNamePart, unionSymbol)
                renderAsVariant(writer, model, symbolProvider, member, variantName, funcNamePart, unionSymbol)
                rust("/// Returns true if this is a [`$variantName`](#T::$variantName).", unionSymbol)
                rust("/// :rtype bool:")
                rustBlock("pub fn is_$funcNamePart(&self) -> bool") {
                    rust("self.0.is_$funcNamePart()")
                }
            }
            if (renderUnknownVariant) {
                rust("/// Returns true if the union instance is the `Unknown` variant.")
                rust("/// :rtype bool:")
                rustBlock("pub fn is_unknown(&self) -> bool") {
                    rust("matches!(self.0, Self::Unknown)")
                }
            }
        }
    }

    private fun renderNewVariant(
        writer: RustWriter,
        model: Model,
        symbolProvider: SymbolProvider,
        member: MemberShape,
        variantName: String,
        funcNamePart: String,
        unionSymbol: Symbol,
    ) {
        if (member.isTargetUnit()) {
            Attribute("staticmethod").render(writer)
            writer.rust(
                "/// Creates a new union instance of [`$variantName`](#T::$variantName)",
                unionSymbol,
            )
            writer.rust("/// :rtype ${unionSymbol.name}:")
            writer.rustBlock("pub fn $funcNamePart() -> Self") {
                rust("Self(${unionSymbol.name}::$variantName")
            }
        } else {
            val memberSymbol = symbolProvider.toSymbol(member)
            val pythonType = memberSymbol.rustType().pythonType()
            val targetType = memberSymbol.rustType()
            Attribute("staticmethod").render(writer)
            writer.rust(
                "/// Creates a new union instance of [`$variantName`](#T::$variantName)",
                unionSymbol,
            )
            writer.rust("/// :param data: ${pythonType.renderAsDocstring()}:")
            writer.rust("/// :rtype ${unionSymbol.name}:")
            writer.rustBlock("pub fn $funcNamePart(data: ${targetType.render()}) -> Self") {
                rust("Self(${unionSymbol.name}::$variantName(data))")
            }
        }
    }

    private fun renderAsVariant(
        writer: RustWriter,
        model: Model,
        symbolProvider: SymbolProvider,
        member: MemberShape,
        variantName: String,
        funcNamePart: String,
        unionSymbol: Symbol,
    ) {
        if (member.isTargetUnit()) {
            writer.rust(
                "/// Tries to convert the union instance into [`$variantName`].",
            )
            writer.rust("/// :rtype None:")
            writer.rustBlockTemplate("pub fn as_$funcNamePart(&self) -> #{pyo3}::PyResult<()>", "pyo3" to pyo3) {
                rust(
                    """
                    self.0.as_$funcNamePart().map_err(aws_smithy_http_server_python::PyUnionVariantException::new_err(
                        "${unionSymbol.name} variant is not None"
                    ))
                    """,
                )
            }
        } else {
            val memberSymbol = symbolProvider.toSymbol(member)
            val pythonType = memberSymbol.rustType().pythonType()
            val targetSymbol = symbolProvider.toSymbol(model.expectShape(member.target))
            val rustType = memberSymbol.rustType()
            writer.rust(
                "/// Tries to convert the enum instance into [`$variantName`](#T::$variantName), extracting the inner #D.",
                unionSymbol,
                targetSymbol,
            )
            writer.rust("/// :rtype ${pythonType.renderAsDocstring()}:")
            writer.rustBlockTemplate("pub fn as_$funcNamePart(&self) -> #{pyo3}::PyResult<${rustType.render()}>", "pyo3" to pyo3) {
                rustTemplate(
                    """
                    match self.0.as_$funcNamePart() {
                        Ok(variant) => Ok(variant.clone().into()),
                        Err(_) => Err(#{pyo3}::exceptions::PyValueError::new_err(
                            "${unionSymbol.name} variant is not of type ${memberSymbol.rustType().pythonType().renderAsDocstring()}"
                        )),
                    }
                    """,
                    "pyo3" to pyo3,
                )
            }
        }
    }

    private fun renderPyUnionDeref() {
        writer.rustBlock("impl std::ops::Deref for PyUnionMarker${unionSymbol.name}") {
            rust(
                """
                type Target = ${unionSymbol.name};

                fn deref(&self) -> &Self::Target {
                    &self.0
                }
                """,
            )
        }
    }

    private fun renderPyUnionFrom() {
        writer.rustBlock("impl From<${unionSymbol.name}> for PyUnionMarker${unionSymbol.name}") {
            rust(
                """
                fn from(other: ${unionSymbol.name}) -> Self {
                    PyUnionMarker${unionSymbol.name}(other)
                }
                """,
            )
        }
    }
}
