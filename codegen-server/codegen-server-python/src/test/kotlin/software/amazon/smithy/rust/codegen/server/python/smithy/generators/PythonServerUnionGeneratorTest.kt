/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.python.smithy.testutil.cargoTest
import software.amazon.smithy.rust.codegen.server.python.smithy.testutil.executePythonServerCodegenVisitor
import software.amazon.smithy.rust.codegen.server.python.smithy.testutil.generatePythonServerPluginContext
import kotlin.io.path.appendText

internal class PythonServerUnionGeneratorTest {
    @Test
    fun `boxed recursive union converts to and from Python`() {
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
                value: NativeValue,
            }

            structure EchoOutput {
                @required
                value: NativeValue,
            }

            union NativeValue {
                array: ArrayValue,
                string: String,
            }

            structure ArrayValue {
                @required
                defaultValue: NativeValue,
            }
            """.asSmithyModel()

        val (pluginCtx, testDir) = generatePythonServerPluginContext(model)
        executePythonServerCodegenVisitor(pluginCtx)

        val writer = RustWriter.forModule("model")
        writer.rust(
            """
            ##[test]
            fn boxed_recursive_union_converts_to_and_from_python() {
                use pyo3::{types::IntoPyDict, Python};

                pyo3::prepare_freethreaded_python();

                let value = Python::with_gil(|py| {
                    let globals = [
                        ("NativeValue", py.get_type::<PyUnionMarkerNativeValue>()),
                        ("ArrayValue", py.get_type::<ArrayValue>()),
                    ]
                    .into_py_dict(py);
                    let locals = pyo3::types::PyDict::new(py);

                    py.run(
                        "leaf = NativeValue.string('leaf')\narray = ArrayValue(leaf)\nassert array.default_value.as_string() == 'leaf'\nvalue = NativeValue.array(array)\nassert value.as_array().default_value.as_string() == 'leaf'",
                        Some(globals),
                        Some(locals),
                    )
                    .unwrap();

                    locals
                        .get_item("value")
                        .unwrap()
                        .unwrap()
                        .extract::<NativeValue>()
                        .unwrap()
                });

                match value {
                    NativeValue::Array(array) => match *array.default_value {
                        NativeValue::String(value) => assert_eq!(value, "leaf"),
                        other => panic!("expected string variant, got {other:?}"),
                    },
                    other => panic!("expected array variant, got {other:?}"),
                }
            }
            """.trimIndent(),
        )

        testDir.resolve("src/model.rs").appendText(writer.toString())

        cargoTest(testDir)
    }
}
