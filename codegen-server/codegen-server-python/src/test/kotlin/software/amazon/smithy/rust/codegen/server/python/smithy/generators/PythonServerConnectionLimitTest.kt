/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.python.smithy.testutil.cargoTest
import software.amazon.smithy.rust.codegen.server.python.smithy.testutil.executePythonServerCodegenVisitor
import software.amazon.smithy.rust.codegen.server.python.smithy.testutil.generatePythonServerPluginContext
import kotlin.io.path.appendText

internal class PythonServerConnectionLimitTest {
    @Test
    fun `Python run exposes an optional limit and rejects invalid values before starting workers`() {
        val model =
            """
            namespace test
            use aws.protocols#restJson1

            @restJson1
            service SampleService {
                operations: [Health]
            }

            @http(method: "GET", uri: "/health")
            operation Health {
                input := {}
                output := {}
            }
            """.asSmithyModel(smithyVersion = "2")

        val (pluginCtx, testDir) = generatePythonServerPluginContext(model)
        executePythonServerCodegenVisitor(pluginCtx)
        testDir.resolve("src/python_server_application.rs").appendText(
            """

            #[cfg(test)]
            mod connection_limit_tests {
                use super::App;
                use pyo3::prelude::*;
                use pyo3::types::IntoPyDict;

                #[test]
                fn python_connection_limit_configuration() {
                    pyo3::prepare_freethreaded_python();
                    Python::with_gil(|py| {
                        let app = Py::new(py, App::default()).unwrap();
                        let locals = [("app", app)].into_py_dict(py);
                        py.run(
                            r#"
            import inspect
            assert inspect.signature(app.run).parameters["max_connections"].default is None
            assert inspect.signature(app.start_worker).parameters["max_connections"].default is None
            # These must fail before binding a socket or spawning any worker.
            for limit in (0, 2**63):
                try:
                    app.run(max_connections=limit)
                except ValueError as error:
                    assert "max_connections" in str(error)
                else:
                    raise AssertionError("invalid connection limit was accepted")
            "#,
                            None,
                            Some(locals),
                        ).unwrap();
                    });
                }
            }
            """.trimIndent(),
        )
        cargoTest(testDir)
    }
}
