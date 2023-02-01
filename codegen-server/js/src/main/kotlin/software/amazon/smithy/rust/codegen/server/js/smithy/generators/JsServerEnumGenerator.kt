/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.js.smithy.generators

import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.server.js.smithy.JsServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerEnumGenerator

/**
 * To share enums defined in Rust with Python, `pyo3` provides the `PyClass` trait.
 * This class generates enums definitions, implements the `PyClass` trait and adds
 * some utility functions like `__str__()` and `__repr__()`.
 */
class JsServerEnumGenerator(
    codegenContext: ServerCodegenContext,
    private val writer: RustWriter,
    shape: StringShape,
) : ServerEnumGenerator(codegenContext, writer, shape) {

    private val napi_derive = JsServerCargoDependency.NapiDerive.toType()
    // override val meta = RustMedata

    override fun render() {
        writer.rust("use napi::bindgen_prelude::ToNapiValue;")
        renderNapi()
        super.render()
    }

    private fun renderNapi() {
        Attribute(napi_derive.resolve("napi")).render(writer)
    }
}
