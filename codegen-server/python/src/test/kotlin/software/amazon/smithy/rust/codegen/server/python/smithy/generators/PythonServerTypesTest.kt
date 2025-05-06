/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.python.smithy.testutil.cargoTest
import software.amazon.smithy.rust.codegen.server.python.smithy.testutil.executePythonServerCodegenVisitor
import software.amazon.smithy.rust.codegen.server.python.smithy.testutil.generatePythonServerPluginContext
import kotlin.io.path.appendText

internal class PythonServerTypesTest {
    private fun getModel(
        structures: String,
        vararg errors: String = emptyArray(),
    ) = """
            namespace test

            use aws.protocols#restJson1
            use smithy.framework#ValidationException

            @restJson1
            service Service {
                operations: [
                    Echo,
                ],
            }

            @http(method: "POST", uri: "/echo")
            operation Echo {
                input: EchoInput,
                output: EchoOutput,
                ${if (errors.isNotEmpty()) "errors: [${errors.joinToString(", ")}]," else ""}
            }

            $structures
            """.asSmithyModel()

    @Test
    fun `document type`() {
        val model =
            getModel(
                """
            structure EchoInput {
                value: Document,
            }
            structure EchoOutput {
                value: Document,
            }
            """,
            )

        val (pluginCtx, testDir) = generatePythonServerPluginContext(model)
        executePythonServerCodegenVisitor(pluginCtx)

        val testCases =
            listOf(
                Pair(
                    """ { "value": 42 } """,
                    """
                    assert input.value == 42
                    output = EchoOutput(value=input.value)
                    """,
                ),
                Pair(
                    """ { "value": "foobar" } """,
                    """
                    assert input.value == "foobar"
                    output = EchoOutput(value=input.value)
                    """,
                ),
                Pair(
                    """
                    {
                        "value": [
                            true,
                            false,
                            42,
                            42.0,
                            -42,
                            {
                                "nested": "value"
                            },
                            {
                                "nested": [1, 2, 3]
                            }
                        ]
                    }
                    """,
                    """
                    assert input.value == [True, False, 42, 42.0, -42, {"nested": "value"}, {"nested": [1, 2, 3]}]
                    output = EchoOutput(value=input.value)
                    """,
                ),
            )

        val writer = RustWriter.forModule("service")
        writer.tokioTest("document_type") {
            rust(
                """
                use tower::Service as _;
                use pyo3::{types::IntoPyDict, IntoPy, Python};
                use hyper::{Body, Request, body};
                use crate::{input, output};

                pyo3::prepare_freethreaded_python();
                """.trimIndent(),
            )

            testCases.forEach {
                val payload = it.first.replace(" ", "").replace("\n", "")
                val pythonHandler = it.second.trimIndent()
                rust(
                    """
                    let mut service = Service::builder_without_plugins()
                        .echo(|input: input::EchoInput| async {
                            Ok(Python::with_gil(|py| {
                                let globals = [("EchoOutput", py.get_type::<output::EchoOutput>())].into_py_dict(py);
                                let locals = [("input", input.into_py(py))].into_py_dict(py);

                                py.run(${pythonHandler.dq()}, Some(globals), Some(locals)).unwrap();

                                locals
                                    .get_item("output")
                                    .unwrap()
                                    .extract::<output::EchoOutput>()
                                    .unwrap()
                            }))
                        })
                        .build()
                        .unwrap();

                    let req = Request::builder()
                        .method("POST")
                        .uri("/echo")
                        .header("content-type", "application/json")
                        .body(Body::from(${payload.dq()}))
                        .unwrap();

                    let res = service.call(req).await.unwrap();
                    assert!(res.status().is_success());
                    let body = body::to_bytes(res.into_body()).await.unwrap();
                    assert_eq!(body, ${payload.dq()});
                    """.trimIndent(),
                )
            }
        }

        testDir.resolve("src/service.rs").appendText(writer.toString())

        cargoTest(testDir)
    }

    @Test
    fun `timestamp type`() {
        val model =
            getModel(
                """
            structure EchoInput {
                @required
                value: Timestamp,
                opt_value: Timestamp,
            }

            structure EchoOutput {
                @required
                value: Timestamp,
                opt_value: Timestamp,
            }
            """,
                "ValidationException",
            )

        val (pluginCtx, testDir) = generatePythonServerPluginContext(model)
        executePythonServerCodegenVisitor(pluginCtx)

        val writer = RustWriter.forModule("service")
        writer.tokioTest("timestamp_type") {
            rust(
                """
                use tower::Service as _;
                use pyo3::{types::IntoPyDict, IntoPy, Python};
                use hyper::{Body, Request, body};
                use crate::{input, output, python_types};

                pyo3::prepare_freethreaded_python();

                let mut service = Service::builder_without_plugins()
                    .echo(|input: input::EchoInput| async {
                        Ok(Python::with_gil(|py| {
                            let globals = [
                                ("EchoOutput", py.get_type::<output::EchoOutput>()),
                                ("DateTime", py.get_type::<python_types::DateTime>()),
                            ].into_py_dict(py);
                            let locals = [("input", input.into_py(py))].into_py_dict(py);

                            py.run("assert input.value.secs() == 1676298520", Some(globals), Some(locals)).unwrap();
                            py.run("output = EchoOutput(value=input.value, opt_value=DateTime.from_secs(1677771678))", Some(globals), Some(locals)).unwrap();

                            locals
                                .get_item("output")
                                .unwrap()
                                .extract::<output::EchoOutput>()
                                .unwrap()
                        }))
                    })
                    .build()
                    .unwrap();

                let req = Request::builder()
                    .method("POST")
                    .uri("/echo")
                    .header("content-type", "application/json")
                    .body(Body::from("{\"value\":1676298520}"))
                    .unwrap();
                let res = service.call(req).await.unwrap();
                assert!(res.status().is_success());
                let body = body::to_bytes(res.into_body()).await.unwrap();
                let body = std::str::from_utf8(&body).unwrap();
                assert!(body.contains("\"value\":1676298520"));
                assert!(body.contains("\"opt_value\":1677771678"));
                """.trimIndent(),
            )
        }

        testDir.resolve("src/service.rs").appendText(writer.toString())

        cargoTest(testDir)
    }

    @Test
    fun `unnamed enum type`() {
        val model =
            getModel(
                """
            structure EchoInput {
                @required
                unnamedValue: Choice,
                unnamedOptValue: Choice,
                @required
                namedValue: Choice,
                namedOptValue: Choice,
            }

            structure EchoOutput {
                @required
                unnamedValue: Choice,
                unnamedOptValue: Choice,
                @required
                namedValue: Choice,
                namedOptValue: Choice,
            }

            @enum([
                {
                    value: "t2.nano",
                },
                {
                    value: "t2.micro",
                },
                {
                    value: "m256.mega",
                    deprecated: true
                }
            ])
            string Choice
            """,
                "ValidationException",
            )

        val (pluginCtx, testDir) = generatePythonServerPluginContext(model)
        executePythonServerCodegenVisitor(pluginCtx)

        cargoTest(testDir)
    }
}
