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
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.isCopy
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
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext

/*
 * Generate unions that are compatible with Python by wrapping the Rust implementation into
 * a new structure and implementing `IntoPy` and `FromPyObject` to ensure the ability to extract
 * the union inside the Python context.
 */
class PythonServerUnionGenerator(
    model: Model,
    private val codegenContext: ServerCodegenContext,
    private val writer: RustWriter,
    shape: UnionShape,
    private val renderUnknownVariant: Boolean = true,
) : UnionGenerator(model, codegenContext.symbolProvider, writer, shape, renderUnknownVariant) {
    private val symbolProvider = codegenContext.symbolProvider
    private val libName = codegenContext.settings.moduleName.toSnakeCase()
    private val sortedMembers: List<MemberShape> = shape.allMembers.values.sortedBy { symbolProvider.toMemberName(it) }
    private val unionSymbol = symbolProvider.toSymbol(shape)

    private val pyo3 = PythonServerCargoDependency.PyO3.toType()

    override fun render() {
        super.render()
        renderPyUnionStruct()
        renderPyUnionImpl()
        renderPyObjectConverters()
    }

    private fun renderPyUnionStruct() {
        writer.rust("""##[pyo3::pyclass(name = "${unionSymbol.name}")]""")
        val containerMeta = unionSymbol.expectRustMetadata()
        containerMeta.render(writer)
        writer.rust("struct PyUnionMarker${unionSymbol.name}(pub ${unionSymbol.name});")
    }

    private fun renderPyUnionImpl() {
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
                    rust("self.0.is_unknown()")
                }
            }
        }
    }

    private fun renderPyObjectConverters() {
        writer.rustBlockTemplate("impl #{pyo3}::IntoPy<#{pyo3}::PyObject> for ${unionSymbol.name}", "pyo3" to pyo3) {
            rustBlockTemplate("fn into_py(self, py: #{pyo3}::Python<'_>) -> #{pyo3}::PyObject", "pyo3" to pyo3) {
                rust("PyUnionMarker${unionSymbol.name}(self).into_py(py)")
            }
        }
        writer.rustBlockTemplate("impl<'source> #{pyo3}::FromPyObject<'source> for ${unionSymbol.name}", "pyo3" to pyo3) {
            rustBlockTemplate("fn extract(obj: &'source #{pyo3}::PyAny) -> #{pyo3}::PyResult<Self>", "pyo3" to pyo3) {
                rust(
                    """
                    let data: PyUnionMarker${unionSymbol.name} = obj.extract()?;
                    Ok(data.0)
                    """,
                )
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
                rust("Self(${unionSymbol.name}::$variantName)")
            }
        } else {
            val memberSymbol = symbolProvider.toSymbol(member)
            val pythonType = memberSymbol.rustType().pythonType(libName)
            val targetType = memberSymbol.rustType()
            Attribute("staticmethod").render(writer)
            writer.rust(
                "/// Creates a new union instance of [`$variantName`](#T::$variantName)",
                unionSymbol,
            )
            writer.rust("/// :param data ${pythonType.renderAsDocstring()}:")
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
                "/// Tries to convert the enum instance into [`$variantName`](#T::$variantName), extracting the inner `()`.",
                unionSymbol,
            )
            writer.rust("/// :rtype None:")
            writer.rustBlockTemplate("pub fn as_$funcNamePart(&self) -> #{pyo3}::PyResult<()>", "pyo3" to pyo3) {
                rustTemplate(
                    """
                    self.0.as_$funcNamePart().map_err(|_| #{pyo3}::exceptions::PyValueError::new_err(
                        "${unionSymbol.name} variant is not None"
                    ))
                    """,
                    "pyo3" to pyo3,
                )
            }
        } else {
            val memberSymbol = symbolProvider.toSymbol(member)
            val pythonType = memberSymbol.rustType().pythonType(libName)
            val targetSymbol = symbolProvider.toSymbol(model.expectShape(member.target))
            val rustType = memberSymbol.rustType()
            writer.rust(
                "/// Tries to convert the enum instance into [`$variantName`](#T::$variantName), extracting the inner #D.",
                unionSymbol,
                targetSymbol,
            )
            writer.rust("/// :rtype ${pythonType.renderAsDocstring()}:")
            writer.rustBlockTemplate("pub fn as_$funcNamePart(&self) -> #{pyo3}::PyResult<${rustType.render()}>", "pyo3" to pyo3) {
                val variantType =
                    if (rustType.isCopy()) {
                        "*variant"
                    } else {
                        "variant.clone()"
                    }
                val errorVariant = memberSymbol.rustType().pythonType(libName).renderAsDocstring()
                rustTemplate(
                    """
                    match self.0.as_$funcNamePart() {
                        Ok(variant) => Ok($variantType),
                        Err(_) => Err(#{pyo3}::exceptions::PyValueError::new_err(
                            r"${unionSymbol.name} variant is not of type $errorVariant"
                        )),
                    }
                    """,
                    "pyo3" to pyo3,
                )
            }
        }
    }
}
