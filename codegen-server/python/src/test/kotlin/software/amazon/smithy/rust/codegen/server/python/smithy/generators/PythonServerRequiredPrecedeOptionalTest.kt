/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.python.smithy.testutil.cargoTest
import software.amazon.smithy.rust.codegen.server.python.smithy.testutil.executePythonServerCodegenVisitor
import software.amazon.smithy.rust.codegen.server.python.smithy.testutil.generatePythonServerPluginContext
import kotlin.io.path.appendText

internal class PythonServerRequiredPrecedeOptionalTest {
    @Test
    fun `mandatory fields are reordered to be before optional`() {
        val model =
            """
            namespace test

            use aws.protocols#restJson1
            use smithy.framework#ValidationException

            @restJson1
            service SampleService {
                operations: [
                    OpWithIncorrectOrder, OpWithCorrectOrder, OpWithDefaults
                ],
            }

            @http(method: "POST", uri: "/opIncorrect")
            operation OpWithIncorrectOrder {
                input:= {
                    a: String
                    @required
                    b: String
                    c: String
                    @required
                    d: String
                }
                output:= {
                    a: String
                    @required
                    b: String
                    c: String
                    @required
                    d: String
                }
                errors: [ValidationException]
            }

            @http(method: "POST", uri: "/opCorrect")
            operation OpWithCorrectOrder {
                input:= {
                    @required
                    b: String
                    @required
                    d: String
                    a: String
                    c: String
                }
                output:= {
                    @required
                    b: String
                    @required
                    d: String
                    a: String
                    c: String
                }
                errors: [ValidationException]
            }

            @http(method: "POST", uri: "/opWithDefaults")
            operation OpWithDefaults {
                input:= {
                a: String,
                b: String = "hi"
                }
            }
            """.asSmithyModel(smithyVersion = "2")

        val (pluginCtx, testDir) = generatePythonServerPluginContext(model)
        executePythonServerCodegenVisitor(pluginCtx)

        val writer = RustWriter.forModule("service")
        writer.unitTest("test_required_fields") {
            rust(
                """
                use crate::input;
                use pyo3::{types::IntoPyDict, Python};

                pyo3::prepare_freethreaded_python();
                Python::with_gil(|py| {
                    let globals = [
                        (
                            "OpWithIncorrectOrderInput",
                            py.get_type::<input::OpWithIncorrectOrderInput>(),
                        ),
                        (
                            "OpWithCorrectOrderInput",
                            py.get_type::<input::OpWithCorrectOrderInput>(),
                        ),
                        (
                            "OpWithDefaultsInput",
                            py.get_type::<input::OpWithDefaultsInput>(),
                        )]
                        .into_py_dict(py);
                    let locals = [(
                        "OpWithIncorrectOrderInput",
                        py.get_type::<input::OpWithIncorrectOrderInput>(),
                    )]
                    .into_py_dict(py);

                    py.run(
                        "input = OpWithIncorrectOrderInput(\"b\", \"d\")",
                        Some(globals),
                        Some(locals),
                    )
                    .unwrap();

                    // Python should have been able to construct input.
                    let input = locals
                        .get_item("input")
                        .expect("Python exception occurred during dictionary lookup")
                        .unwrap()
                        .extract::<input::OpWithIncorrectOrderInput>()
                        .unwrap();
                    assert_eq!(input.b, "b");
                    assert_eq!(input.d, "d");

                    py.run(
                        "input = OpWithCorrectOrderInput(\"b\", \"d\")",
                        Some(globals),
                        Some(locals),
                    )
                    .unwrap();

                    // Python should have been able to construct input.
                    let input = locals
                        .get_item("input")
                        .expect("Python exception occurred during dictionary lookup")
                        .unwrap()
                        .extract::<input::OpWithCorrectOrderInput>()
                        .unwrap();
                    assert_eq!(input.b, "b");
                    assert_eq!(input.d, "d");

                    py.run(
                        "input = OpWithDefaultsInput(\"a\")",
                        Some(globals),
                        Some(locals),
                    )
                    .unwrap();

                    // KanchaBilla
                    // Python should have been able to construct input.
                    let input = locals
                        .get_item("input")
                        .expect("Python exception occurred during dictionary lookup")
                        .unwrap()
                        .extract::<input::OpWithDefaultsInput>()
                        .unwrap();
                    assert_eq!(input.a, Some("a".to_string()));
                    assert_eq!(input.b, "hi");
                });
                """,
            )
        }

        testDir.resolve("src/service.rs").appendText(writer.toString())

        cargoTest(testDir)
    }
}
