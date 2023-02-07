/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.typescript.smithy.generators

import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerEnumGenerator
import software.amazon.smithy.rust.codegen.server.typescript.smithy.TsServerCargoDependency

/**
 * To share enums defined in Rust with Typescript, `napi-rs` provides the `napi` trait.
 * This class generates enums definitions and implements the `napi` trait.
 */
class TsServerEnumGenerator(
    codegenContext: ServerCodegenContext,
    private val writer: RustWriter,
    shape: StringShape,
) : ServerEnumGenerator(codegenContext, writer, shape) {

    private val napi_derive = TsServerCargoDependency.NapiDerive.toType()

    override fun render() {
        writer.rust("use napi::bindgen_prelude::ToNapiValue;")
        Attribute(napi_derive.resolve("napi")).render(writer)
        super.render()
    }
}
