/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.js.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustInlineTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.server.js.smithy.JsServerCargoDependency

/**
 * To share structures defined in Rust with Python, `pyo3` provides the `PyClass` trait.
 * This class generates input / output / error structures definitions and implements the
 * `PyClass` trait.
 */
class JsServerStructureGenerator(
    model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val writer: RustWriter,
    private val shape: StructureShape,
) : StructureGenerator(model, symbolProvider, writer, shape) {

    private val napi_derive = JsServerCargoDependency.NapiDerive.toType()

    override fun renderStructure() {
        val flavour = if (shape.hasTrait<ErrorTrait>()) {
            "constructor"
        } else {
            "object"
        }
        Attribute(
            writable {
                rustInlineTemplate(
                    "#{napi}($flavour)",
                    "napi" to napi_derive.resolve("napi"),
                )
            },
        ).render(writer)
        super.renderStructure()
    }
}
