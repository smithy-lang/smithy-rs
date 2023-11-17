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

    private val pyO3 = PythonServerCargoDependency.PyO3.toType()
    private val codegenScope =
        arrayOf(
            "Inner" to innerT,
            "Error" to errorT,
            "SmithyPython" to PythonServerCargoDependency.smithyHttpServerPython(runtimeConfig).toType(),
            "SmithyHttp" to RuntimeType.smithyHttp(runtimeConfig),
            "Tracing" to PythonServerCargoDependency.Tracing.toType(),
            "PyO3" to pyO3,
            "PyO3Asyncio" to PythonServerCargoDependency.PyO3Asyncio.toType(),
            "TokioStream" to PythonServerCargoDependency.TokioStream.toType(),
            "Mutex" to PythonServerCargoDependency.ParkingLot.toType().resolve("Mutex"),
            "AsyncMutex" to PythonServerCargoDependency.Tokio.toType().resolve("sync::Mutex"),
            "Send" to RuntimeType.Send,
            "Sync" to RuntimeType.Sync,
            "Option" to RuntimeType.Option,
            "Arc" to RuntimeType.Arc,
            "Body" to RuntimeType.sdkBody(runtimeConfig),
            "UnmarshallMessage" to RuntimeType.smithyEventStream(runtimeConfig).resolve("frame::UnmarshallMessage"),
            "MarshallMessage" to RuntimeType.smithyEventStream(runtimeConfig).resolve("frame::MarshallMessage"),
            "SignMessage" to RuntimeType.smithyEventStream(runtimeConfig).resolve("frame::SignMessage"),
            "MessageStreamAdapter" to RuntimeType.smithyHttp(runtimeConfig).resolve("event_stream::MessageStreamAdapter"),
            "SdkError" to RuntimeType.sdkError(runtimeConfig),
        )

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
                "inner: #{Arc}<#{Mutex}<#{Option}<#{Wrapped}<#{Inner}, #{Error}>>>>",
                *codegenScope,
                "Wrapped" to wrappedT,
            )
        }

        writer.rustBlock("impl $name") {
            writer.rustTemplate(
                """
                pub fn into_body_stream(
                    self,
                    marshaller: impl #{MarshallMessage}<Input = #{Inner}> + #{Send} + #{Sync} + 'static,
                    error_marshaller: impl #{MarshallMessage}<Input = #{Error}> + #{Send} + #{Sync} + 'static,
                    signer: impl #{SignMessage} + #{Send} + #{Sync} + 'static,
                ) -> #{MessageStreamAdapter}<#{Inner}, #{Error}> {
                    let mut inner = self.inner.lock();
                    let inner = inner.take().expect(
                        "attempted to reuse an event stream. \
                         that means you kept a reference to an event stream and tried to reuse it in another request, \
                         event streams are request scoped and shouldn't be used outside of their bounded request scope"
                    );
                    inner.into_body_stream(marshaller, error_marshaller, signer)
                }
                """,
                *codegenScope,
                "Wrapped" to wrappedT,
            )
        }

        writer.rustTemplate(
            """
            impl<'source> #{PyO3}::FromPyObject<'source> for $name {
                fn extract(obj: &'source #{PyO3}::PyAny) -> #{PyO3}::PyResult<Self> {
                    use #{TokioStream}::StreamExt;
                    let stream = #{PyO3Asyncio}::tokio::into_stream_v1(obj)?;
                    let stream = stream.filter_map(|res| {
                        #{PyO3}::Python::with_gil(|py| {
                            // TODO(EventStreamImprovements): Add `InternalServerError` variant to all event streaming
                            //                                errors and return that variant in case of errors here?
                            match res {
                                Ok(obj) => {
                                    match obj.extract::<#{Inner}>(py) {
                                        Ok(it) => Some(Ok(it)),
                                        Err(err) => {
                                            let rich_py_err = #{SmithyPython}::rich_py_err(err);
                                            #{Tracing}::error!(error = ?rich_py_err, "could not extract the output type '#{Inner}' from streamed value");
                                            None
                                        },
                                    }
                                },
                                Err(err) => {
                                    match #{PyO3}::IntoPy::into_py(err, py).extract::<#{Error}>(py) {
                                        Ok(modelled_error) => Some(Err(modelled_error)),
                                        Err(err) => {
                                            let rich_py_err = #{SmithyPython}::rich_py_err(err);
                                            #{Tracing}::error!(error = ?rich_py_err, "could not extract the error type '#{Error}' from raised exception");
                                            None
                                        }
                                    }
                                }
                            }
                        })
                    });

                    Ok($name { inner: #{Arc}::new(#{Mutex}::new(Some(stream.into()))) })
                }
            }

            impl #{PyO3}::IntoPy<#{PyO3}::PyObject> for $name {
                fn into_py(self, py: #{PyO3}::Python<'_>) -> #{PyO3}::PyObject {
                    #{PyO3}::exceptions::PyAttributeError::new_err("this is a write-only object").into_py(py)
                }
            }
            """,
            *codegenScope,
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
                "inner: #{Arc}<#{AsyncMutex}<#{Wrapped}<#{Inner}, #{Error}>>>",
                *codegenScope,
                "Wrapped" to wrappedT,
            )
        }

        writer.rustBlock("impl $name") {
            writer.rustTemplate(
                """
                pub fn new(
                    unmarshaller: impl #{UnmarshallMessage}<Output = #{Inner}, Error = #{Error}> + #{Send} + #{Sync} + 'static,
                    body: #{Body}
                ) -> $name {
                    let inner = #{Wrapped}::new(unmarshaller, body);
                    let inner = #{Arc}::new(#{AsyncMutex}::new(inner));
                    $name { inner }
                }
                """,
                *codegenScope,
                "Wrapped" to wrappedT,
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
                            Ok(Some(data)) => Ok(#{PyO3}::Python::with_gil(|py| #{PyO3}::IntoPy::into_py(data, py))),
                            Ok(None) => Err(#{PyO3}::exceptions::PyStopAsyncIteration::new_err("stream exhausted")),
                            Err(#{SdkError}::ServiceError(service_err)) => Err(service_err.into_err().into()),
                            Err(err) => Err(#{PyO3}::exceptions::PyRuntimeError::new_err(err.to_string())),
                        }
                    })?;
                    Ok(Some(fut.into()))
                }
                """,
                *codegenScope,
            )
        }
    }
}
