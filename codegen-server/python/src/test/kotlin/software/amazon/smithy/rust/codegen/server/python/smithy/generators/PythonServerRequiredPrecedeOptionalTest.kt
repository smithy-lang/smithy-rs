/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency
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
                output:= {
                    a: String,
                    b: String = "hi"
                }
            }
            """.asSmithyModel(smithyVersion = "2")

        val (pluginCtx, testDir) = generatePythonServerPluginContext(model)
        executePythonServerCodegenVisitor(pluginCtx)

        val writer = RustWriter.forModule("service")
        writer.unitTest("test_required_fields") {
            fun createInstanceWithRequiredFieldsOnly(
                module: String,
                typeName: String,
            ) = writable {
                rustTemplate(
                    """
                    py.run(
                        "data = $typeName(\"b\", \"d\")",
                        Some(globals),
                        Some(locals),
                    ).unwrap();

                    // Python should have been able to construct input.
                    let data = locals
                        .get_item("data")
                        .expect("Python exception occurred during dictionary lookup")
                        .unwrap()
                        .extract::<$module::$typeName>()
                        .unwrap();
                    assert_eq!(data.b, "b");
                    assert_eq!(data.d, "d");
                    """,
                )
            }

            fun createInstance(
                module: String,
                typeName: String,
            ) = writable {
                rustTemplate(
                    """
                    py.run(
                        "data = $typeName(\"b\", \"d\", a = \"a\", c = \"c\")",
                        Some(globals),
                        Some(locals),
                    ).unwrap();

                    // Python should have been able to construct input.
                    let data = locals
                        .get_item("data")
                        .expect("Python exception occurred during dictionary lookup")
                        .unwrap()
                        .extract::<$module::$typeName>()
                        .unwrap();
                    assert_eq!(data.b, "b");
                    assert_eq!(data.d, "d");
                    assert_eq!(data.a, Some("a".to_string()));
                    assert_eq!(data.c, Some("c".to_string()));
                    """,
                )
            }

            fun createDefaultInstance(
                module: String,
                typeName: String,
            ) = writable {
                rustTemplate(
                    """
                    // Default values are not exported from Rust. However, they
                    // are marked as non-optional.
                    py.run(
                        "data = $typeName(\"b\", \"a\")",
                        Some(globals),
                        Some(locals),
                    ).unwrap();

                    // Python should have been able to construct input.
                    let data = locals
                        .get_item("data")
                        .expect("Python exception occurred during dictionary lookup")
                        .unwrap()
                        .extract::<$module::$typeName>()
                        .unwrap();
                    assert_eq!(data.a, Some("a".to_string()));
                    assert_eq!(data.b, "b");
                    """,
                )
            }

            rustTemplate(
                """
                use crate::{input, output};
                use #{pyo3}::{types::IntoPyDict, Python};

                pyo3::prepare_freethreaded_python();
                Python::with_gil(|py| {
                    let globals = [
                        ("OpWithIncorrectOrderInput", py.get_type::<input::OpWithIncorrectOrderInput>()),
                        ("OpWithCorrectOrderInput", py.get_type::<input::OpWithCorrectOrderInput>()),
                        ("OpWithDefaultsInput", py.get_type::<input::OpWithDefaultsInput>()),
                        ("OpWithIncorrectOrderOutput", py.get_type::<output::OpWithIncorrectOrderOutput>()),
                        ("OpWithCorrectOrderOutput", py.get_type::<output::OpWithCorrectOrderOutput>()),
                        ("OpWithDefaultsOutput", py.get_type::<output::OpWithDefaultsOutput>())
                        ]
                        .into_py_dict(py);

                    let locals = [("OpWithIncorrectOrderInput", py.get_type::<input::OpWithIncorrectOrderInput>())].into_py_dict(py);

                    #{IncorrectOrderInputRequiredOnly}
                    #{CorrectOrderInputRequiredOnly}
                    #{IncorrectOrderOutputRequiredOnly}
                    #{CorrectOrderOutputRequiredOnly}
                    #{IncorrectOrderInput}
                    #{CorrectOrderInput}
                    #{IncorrectOrderOutput}
                    #{CorrectOrderOutput}
                    #{DefaultsInput}
                    #{DefaultsOutput}
                });
                """,
                "pyo3" to PythonServerCargoDependency.PyO3.toDevDependency().toType(),
                "IncorrectOrderInputRequiredOnly" to createInstanceWithRequiredFieldsOnly("input", "OpWithIncorrectOrderInput"),
                "CorrectOrderInputRequiredOnly" to createInstanceWithRequiredFieldsOnly("input", "OpWithCorrectOrderInput"),
                "IncorrectOrderOutputRequiredOnly" to createInstanceWithRequiredFieldsOnly("output", "OpWithIncorrectOrderOutput"),
                "CorrectOrderOutputRequiredOnly" to createInstanceWithRequiredFieldsOnly("output", "OpWithCorrectOrderOutput"),
                "IncorrectOrderInput" to createInstance("input", "OpWithIncorrectOrderInput"),
                "CorrectOrderInput" to createInstance("input", "OpWithCorrectOrderInput"),
                "IncorrectOrderOutput" to createInstance("output", "OpWithIncorrectOrderOutput"),
                "CorrectOrderOutput" to createInstance("output", "OpWithCorrectOrderOutput"),
                "DefaultsInput" to createDefaultInstance("input", "OpWithDefaultsInput"),
                "DefaultsOutput" to createDefaultInstance("output", "OpWithDefaultsOutput"),
            )
        }

        testDir.resolve("src/service.rs").appendText(writer.toString())

        cargoTest(testDir)
    }
}
