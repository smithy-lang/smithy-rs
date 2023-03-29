/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.transformers.eventStreamErrors
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerOperationErrorGenerator

/**
 * Generates Python compatible error types for event streaming union types.
 * It just uses [ServerOperationErrorGenerator] under the hood to generate pure Rust error types and then adds
 * implementations of `pyo3::FromPyObject` and `pyo3::IntoPy` to allow moving errors from Rust to Python and vice-versa.
 */
class PythonServerEventStreamErrorGenerator(
    private val model: Model,
    private val symbolProvider: RustSymbolProvider,
    val shape: UnionShape,
): ServerOperationErrorGenerator(
    model,
    symbolProvider,
    shape
) {
    private val errorSymbol = symbolProvider.symbolForEventStreamError(shape)
    private val errors = shape.eventStreamErrors().map {
        model.expectShape(it.asMemberShape().get().target, StructureShape::class.java)
    }

    private val pyO3 = PythonServerCargoDependency.PyO3.toType()

    override fun render(writer: RustWriter) {
        super.render(writer)
        renderFromPyObjectImpl(writer)
        renderIntoPyImpl(writer)
    }

    private fun renderFromPyObjectImpl(writer: RustWriter) {
        writer.rustBlockTemplate("impl<'source> #{PyO3}::FromPyObject<'source> for ${errorSymbol.name}", "PyO3" to pyO3) {
            writer.rustBlockTemplate("fn extract(obj: &'source #{PyO3}::PyAny) -> #{PyO3}::PyResult<Self>", "PyO3" to pyO3) {
                errors.forEach {
                    val symbol = symbolProvider.toSymbol(it)
                    writer.rust(
                        """
                        if let Ok(it) = obj.extract::<#T>() {
                            return Ok(Self::${symbol.name}(it));
                        }    
                        """,
                        symbol
                    )
                }
                writer.rustTemplate(
                    """
                    Err(#{PyO3}::exceptions::PyTypeError::new_err(
                        format!(
                            "failed to extract '${errorSymbol.name}' from '{}'",
                            obj
                        )
                    ))
                    """,
                    "PyO3" to pyO3
                )
            }
        }
    }

    private fun renderIntoPyImpl(writer: RustWriter) {
        writer.rustBlockTemplate("impl #{PyO3}::IntoPy<#{PyO3}::PyObject> for ${errorSymbol.name}", "PyO3" to pyO3) {
            writer.rustBlockTemplate("fn into_py(self, py: #{PyO3}::Python<'_>) -> #{PyO3}::PyObject", "PyO3" to pyO3) {
                writer.rustBlock("match self") {
                    errors.forEach {
                        val symbol = symbolProvider.toSymbol(it)
                        writer.rustTemplate(
                            """
                            Self::${symbol.name}(it) => match #{PyO3}::Py::new(py, it) {
                                Ok(it) => it.into_py(py),
                                Err(err) => err.into_py(py),
                            }
                            """,
                            "PyO3" to pyO3
                        )
                    }
                }
            }
        }
    }
}
