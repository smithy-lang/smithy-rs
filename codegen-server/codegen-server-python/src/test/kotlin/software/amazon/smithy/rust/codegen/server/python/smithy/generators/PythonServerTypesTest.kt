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
    @Test
    fun `document type`() {
        val model =
            """
            namespace test

            use aws.protocols#restJson1

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
            }

            structure EchoInput {
                value: Document,
            }

            structure EchoOutput {
                value: Document,
            }
            """.asSmithyModel()

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
                                    .expect("Python exception occurred during dictionary lookup")
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
    fun `big number types are exported and usable from Python`() {
        val model =
            """
            namespace test

            use aws.protocols#restJson1

            @restJson1
            service Service {
                operations: [Echo],
            }

            @http(method: "POST", uri: "/echo")
            operation Echo {
                input: EchoInput,
                output: EchoOutput,
            }

            structure EchoInput {
                @required
                integer: BigInteger,
                @required
                decimal: BigDecimal,
            }

            structure EchoOutput {
                @required
                integer: BigInteger,
                @required
                decimal: BigDecimal,
            }
            """.asSmithyModel()

        val (pluginCtx, testDir) = generatePythonServerPluginContext(model)
        executePythonServerCodegenVisitor(pluginCtx)

        val writer = RustWriter.forModule("service")
        writer.rust(
            """
            ##[test]
            fn big_number_types_are_exported_and_usable_from_python() {
                use pyo3::{types::PyModule, Python};

                pyo3::prepare_freethreaded_python();

                Python::with_gil(|py| {
                    let module = PyModule::new(py, "generated_server").unwrap();
                    crate::python_module_export::python_library(py, module).unwrap();

                    let types = module.getattr("types").unwrap();
                    let exported_types = types
                        .getattr("__all__")
                        .unwrap()
                        .extract::<Vec<String>>()
                        .unwrap();
                    assert!(exported_types.contains(&"BigInteger".to_owned()));
                    assert!(exported_types.contains(&"BigDecimal".to_owned()));

                    let integer = types
                        .getattr("BigInteger")
                        .unwrap()
                        .call1(("123456789012345678901234567890",))
                        .unwrap();
                    let decimal = types
                        .getattr("BigDecimal")
                        .unwrap()
                        .call1(("12345678901234567890.123456789",))
                        .unwrap();

                    let input = module
                        .getattr("input")
                        .unwrap()
                        .getattr("EchoInput")
                        .unwrap()
                        .call1((integer, decimal))
                        .unwrap()
                        .extract::<crate::input::EchoInput>()
                        .unwrap();

                    assert_eq!(input.integer.as_ref(), "123456789012345678901234567890");
                    assert_eq!(input.decimal.as_ref(), "12345678901234567890.123456789");
                });
            }
            """.trimIndent(),
        )

        testDir.resolve("src/service.rs").appendText(writer.toString())

        cargoTest(testDir)
    }

    @Test
    fun `timestamp type`() {
        val model =
            """
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
                errors: [ValidationException],
            }

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
            """.asSmithyModel()

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
                                .expect("Python exception occurred during dictionary lookup")
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
}
