/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.orNull
import software.amazon.smithy.rust.codegen.util.toSnakeCase

/**
 * To share unions defined in Rust with Python, `pyo3` provides the `PyClass` trait.
 * This class generates unions definitions with one wrapper type, implements the
 * `PyClass` trait and adds some utility functions like `__str__()` and * `__repr__()`.
 */
class PythonServerUnionGenerator(
    model: Model,
    symbolProvider: RustSymbolProvider,
    private val writer: RustWriter,
    private val shape: UnionShape,
    private val renderUnknownVariant: Boolean = true,
) : UnionGenerator(model, symbolProvider, writer, shape, renderUnknownVariant) {

    private val pyo3Symbol = PythonServerCargoDependency.PyO3.asType()
    private val unionSymbol = symbolProvider.toSymbol(shape)

    private val wrapperSuffix = "WrapperPrivate__"

    private val codegenScope =
        arrayOf(
            "pyo3" to PythonServerCargoDependency.PyO3.asType(),
            "union" to symbolProvider.toSymbol(shape),
        )

    override fun render() {
        super.render()
        renderPyClass()
        renderWrapper()
        renderWrapperImpl()
    }

    private fun renderPyClass() {
        Attribute.Custom("pyo3::pyclass(name = ${unionSymbol.name.dq()})", symbols = listOf(pyo3Symbol)).render(writer)
    }

    private fun renderWrapper() {
        val containerMeta = unionSymbol.expectRustMetadata()
        containerMeta.render(writer)
        writer.rust("struct ${unionSymbol.name}$wrapperSuffix(${unionSymbol.name});")
    }

    private fun renderWrapperImpl() {
        Attribute.Custom("pyo3::pymethods", symbols = listOf(pyo3Symbol)).render(writer)
        writer.rustBlock("impl ${unionSymbol.name}$wrapperSuffix") {
            rust("/// Constructs an instance of union ${unionSymbol.name}")
            Attribute.Custom("new").render(writer)
            rustBlockTemplate("fn constructor(ty: &#{pyo3}::PyAny) -> #{pyo3}::PyResult<Self>", *codegenScope) {
                rustTemplate("#{pyo3}::Python::with_gil(|py|", *codegenScope)
                withBlock("{ ", "})") {
                    sortedMembers.forEach { member ->
                        val variantName = symbolProvider.toMemberName(member)
                        val memberSymbol = symbolProvider.toSymbol(member)
                        var suffix = ""
                        model.getShape(member.target).orNull().let {
                            if (it != null && it.isUnionShape()) {
                                suffix = wrapperSuffix
                            }
                        }
                        rustBlock("if let Ok(val) = ty.extract::<#T$suffix>()", memberSymbol) {
                            rustTemplate("return Ok(Self(#{union}::$variantName(val)))", *codegenScope)
                        }
                    }
                    val message = "not a valid member value to construct ${unionSymbol.name}"
                    rustTemplate("Err(#{pyo3}::exceptions::PyTypeError::new_err(${message.dq()}))", *codegenScope)
                }
            }

            sortedMembers.forEach { member ->
                val memberSymbol = symbolProvider.toSymbol(member)
                val funcNamePart = member.memberName.toSnakeCase()
                val variantName = symbolProvider.toMemberName(member)

                rustTemplate(
                    "/// Tries to convert the enum instance into [`$variantName`](pyo3::$variantName), extracting the inner member.",
                    *codegenScope,
                    "member" to memberSymbol,
                )
                rust("/// Returns `Err(&Self)` if it can't be converted.")
                rustBlockTemplate("pub fn as_$funcNamePart(&self) -> #{pyo3}::PyResult<#{member}>", *codegenScope, "member" to memberSymbol) {
                    val message = "cannot access $variantName"
                    rustTemplate(
                        "self.0.as_$funcNamePart().map(|x| x.to_owned()).or(Err(#{pyo3}::exceptions::PyAttributeError::new_err(${message.dq()})))",
                        *codegenScope,
                    )
                }
                rustTemplate("/// Returns true if this is a [`$variantName`](#{union}::$variantName).", *codegenScope)

                rustBlock("pub fn is_$funcNamePart(&self) -> bool") {
                    rust("self.0.as_$funcNamePart().is_ok()")
                }
            }

            rust(
                """
                fn __repr__(&self) -> String  {
                    format!("{:?}", self.0)
                }
                fn __str__(&self) -> String {
                    format!("{:?}", self.0)
                }
                """,
            )
        }
    }
}
