/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.typescript.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustInlineTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructSettings
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.server.typescript.smithy.TsServerCargoDependency

/**
 * To share structures defined in Rust with Typescript, `napi-rs` provides the `napi` trait.
 * This class generates input / output / error structures definitions and implements the
 * `napi` trait.
 */
class TsServerStructureGenerator(
    model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val writer: RustWriter,
    private val shape: StructureShape,
) : StructureGenerator(model, symbolProvider, writer, shape, listOf(), StructSettings(flattenVecAccessors = false)) {
    private val napiDerive = TsServerCargoDependency.NapiDerive.toType()

    override fun renderStructure() {
        val flavour =
            if (shape.hasTrait<ErrorTrait>()) {
                "constructor"
            } else {
                "object"
            }
        Attribute(
            writable {
                rustInlineTemplate(
                    "#{napi}($flavour)",
                    "napi" to napiDerive.resolve("napi"),
                )
            },
        ).render(writer)
        super.renderStructure()
    }
}
