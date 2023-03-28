/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.util.isOutputEventStream
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonEventStreamSymbolProvider
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency

/**
 * Generates Python wrapper types for event streaming members.
 * In pure Rust we use `aws_smithy_http::event_stream::{Receiver,EventStreamSender}<T, E>` for event streaming members,
 * this is not viable for Python because PyO3 expects following for a type to be exposed to Python, but we fail to satisfy:
 *  - It should be `Clone` and `Send`
 *  - It shouldn't have any generic parameters
 *
 *  So we generate wrapper types for every streaming member, that looks like:
 *  ```rust
 *  #[pyo3::pyclass]
 *  #[derive(std::clone::Clone, std::fmt::Debug)]
 *  pub struct CapturePokemonInputEventsReceiver {
 *      // Arc makes it cloneable
 *      inner: std::sync::Arc<
 *          // Mutex makes it sendable
 *          tokio::sync::Mutex<
 *              // Filling generic args with specific impls make outer type non-generic
 *              aws_smithy_http::event_stream::Receiver<
 *                  crate::model::AttemptCapturingPokemonEvent,
 *                  crate::error::AttemptCapturingPokemonEventError,
 *              >,
 *          >,
 *      >,
 *  }
 *  ```
 *
 *  For Receiver and Sender types we also implement:
 *      Receiver: `__anext__` so it can be used in async for loops in Python
 *      Sender: `FromPyObject` that converts an async Python generator to a Rust stream
 */
class PythonServerEventStreamWrapperGenerator(
    codegenContext: CodegenContext,
    private val shape: MemberShape,
) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider

    private val symbol = symbolProvider.toSymbol(shape)
    private val eventStreamSymbol = PythonEventStreamSymbolProvider.parseSymbol(symbol)
    private val innerT = eventStreamSymbol.innerT
    private val errorT = eventStreamSymbol.errorT
    private val containerName = shape.container.name
    private val memberName = shape.memberName.toPascalCase()

    private val smithyHttp = RuntimeType.smithyHttp(runtimeConfig)
    private val smithyPython = PythonServerCargoDependency.smithyHttpServerPython(runtimeConfig).toType()
    private val pyO3 = PythonServerCargoDependency.PyO3.toType()
    private val pyO3Asyncio = PythonServerCargoDependency.PyO3Asyncio.toType()
    private val tokio = PythonServerCargoDependency.Tokio.toType()
    private val futures = PythonServerCargoDependency.Futures.toType()
    private val parkingLot = PythonServerCargoDependency.ParkingLot.toType()
    private val tracing = PythonServerCargoDependency.Tracing.toType()

    fun render(writer: RustWriter) {
        if (shape.isOutputEventStream(model)) {
            renderSender(writer)
        } else {
            renderReceiver(writer)
        }
    }

    private fun renderSender(writer: RustWriter) {
        val name = "${containerName}${memberName}EventStreamSender"
        val wrappedT = RuntimeType.eventStreamSender(runtimeConfig)
        val containerMeta = symbol.expectRustMetadata().withDerives(RuntimeType.Clone, RuntimeType.Debug)
        containerMeta.render(writer)
        writer.rustBlock("struct $name") {
            writer.rustTemplate(
                "inner: std::sync::Arc<#{Mutex}<Option<#{Wrapped}<#{Inner}, #{Error}>>>>",
                "Mutex" to parkingLot.resolve("Mutex"),
                "Wrapped" to wrappedT,
                "Inner" to innerT,
                "Error" to errorT,
            )
        }

        writer.rustBlock("impl $name") {
            writer.rustTemplate(
                """
                pub fn into_body_stream(
                    self,
                    marshaller: impl #{MarshallMessage}<Input = #{Output}> + Send + Sync + 'static,
                    error_marshaller: impl #{MarshallMessage}<Input = #{Error}> + Send + Sync + 'static,
                    signer: impl #{SignMessage} + Send + Sync + 'static,
                ) -> #{MessageStreamAdapter}<#{Output}, #{Error}> {
                    let mut inner = self.inner.lock();
                    let inner = inner.take().unwrap();
                    inner.into_body_stream(marshaller, error_marshaller, signer)
                }
                """,
                "MarshallMessage" to RuntimeType.smithyEventStream(runtimeConfig).resolve("frame::MarshallMessage"),
                "SignMessage" to RuntimeType.smithyEventStream(runtimeConfig).resolve("frame::SignMessage"),
                "MessageStreamAdapter" to RuntimeType.smithyHttp(runtimeConfig).resolve("event_stream::MessageStreamAdapter"),
                "Body" to RuntimeType.sdkBody(runtimeConfig),
                "Wrapped" to wrappedT,
                "Output" to innerT,
                "Error" to errorT,
            )
        }

        writer.rustTemplate(
            """
            impl<'source> #{PyO3}::FromPyObject<'source> for $name {
                fn extract(obj: &'source #{PyO3}::PyAny) -> #{PyO3}::PyResult<Self> {
                    use #{Futures}::StreamExt;
                    let stream = #{PyO3Asyncio}::tokio::into_stream_v2(obj)?;
                    let stream = stream.filter_map(|obj| {
                        #{Futures}::future::ready(#{PyO3}::Python::with_gil(|py| {
                            if let Ok(err) = obj.extract::<#{Error}>(py) {
                                return Some(Err(err));
                            }

                            if let Ok(res) = obj.extract::<#{Inner}>(py) {
                                return Some(Ok(res));
                            }

                            // TODO: Add `InternalServerError` variant to all event streaming errors and return that variant here? 
                            #{Tracing}::error!(value = ?obj, "could not extract '#{Inner}' or '#{Error}' from streamed value");
                            None
                        }))
                    });
            
                    Ok($name { inner: std::sync::Arc::new(#{Mutex}::new(Some(stream.into()))) })
                }
            }
            
            impl #{PyO3}::IntoPy<#{PyO3}::PyObject> for $name {
                fn into_py(self, py: #{PyO3}::Python<'_>) -> #{PyO3}::PyObject {
                    #{PyO3}::exceptions::PyAttributeError::new_err("this is a write-only field").into_py(py)
                }
            }
            """,
            "Mutex" to parkingLot.resolve("Mutex"),
            "SmithyPython" to smithyPython,
            "Tracing" to tracing,
            "PyO3" to pyO3,
            "PyO3Asyncio" to pyO3Asyncio,
            "Error" to errorT,
            "Inner" to innerT,
            "Futures" to futures,
        )
    }

    private fun renderReceiver(writer: RustWriter) {
        val name = "${containerName}${memberName}Receiver"
        val wrappedT = RuntimeType.eventStreamReceiver(runtimeConfig)
        val containerMeta = symbol.expectRustMetadata().withDerives(RuntimeType.Clone, RuntimeType.Debug)
        Attribute(pyO3.resolve("pyclass")).render(writer)
        containerMeta.render(writer)
        writer.rustBlock("struct $name") {
            writer.rustTemplate(
                "inner: std::sync::Arc<#{Mutex}<#{Wrapped}<#{Inner}, #{Error}>>>",
                "Mutex" to tokio.resolve("sync::Mutex"),
                "Wrapped" to wrappedT,
                "Inner" to innerT,
                "Error" to errorT,
            )
        }

        writer.rustBlock("impl $name") {
            writer.rustTemplate(
                """
                pub fn new(
                    unmarshaller: impl #{UnmarshallMessage}<Output = #{Output}, Error = #{Error}> + Send + Sync + 'static, 
                    body: #{Body}
                ) -> $name {
                    let inner = #{Wrapped}::new(unmarshaller, body);
                    let inner = std::sync::Arc::new(#{Mutex}::new(inner));
                    $name { inner }
                }
                """,
                "Mutex" to tokio.resolve("sync::Mutex"),
                "UnmarshallMessage" to RuntimeType.smithyEventStream(runtimeConfig).resolve("frame::UnmarshallMessage"),
                "Body" to RuntimeType.sdkBody(runtimeConfig),
                "Wrapped" to wrappedT,
                "Output" to innerT,
                "Error" to errorT,
            )
        }

        Attribute(pyO3.resolve("pymethods")).render(writer)
        writer.rustBlock("impl $name") {
            writer.rustTemplate(
                """
                pub fn __aiter__(slf: #{PyO3}::PyRef<Self>) -> #{PyO3}::PyRef<Self> {
                    slf
                }
                
                pub fn __anext__(slf: #{PyO3}::PyRefMut<Self>) -> #{PyO3}::PyResult<Option<#{PyO3}::PyObject>> {
                    let body = slf.inner.clone();
                    let fut = #{PyO3Asyncio}::tokio::future_into_py(slf.py(), async move {
                        let mut inner = body.lock().await;
                        let next = inner.recv().await;
                        match next {
                            Ok(Some(data)) => Ok(#{PyO3}::Python::with_gil(|py| pyo3::IntoPy::into_py(data, py))),
                            Ok(None) => Err(#{PyO3}::exceptions::PyStopAsyncIteration::new_err("stream exhausted")),
                            // TODO: Modelled error should implement `IntoPy` and we should return modelled error here instead of terminating the stream.
                            // Err(#{SmithyHttp}::result::SdkError::ServiceError(service_err)) => Ok(#{PyO3}::Python::with_gil(|py| pyo3::IntoPy::into_py(service_err.into_err(), py))), 
                            Err(#{SmithyHttp}::result::SdkError::ServiceError(_service_err)) => Err(#{PyO3}::exceptions::PyStopAsyncIteration::new_err("stream exhausted")), 
                            Err(err) => Err(#{PyO3}::exceptions::PyRuntimeError::new_err(err.to_string())),
                        }
                    })?;
                    Ok(Some(fut.into()))
                }
                """,
                "PyO3" to pyO3,
                "PyO3Asyncio" to pyO3Asyncio,
                "SmithyHttp" to smithyHttp,
            )
        }
    }
}
