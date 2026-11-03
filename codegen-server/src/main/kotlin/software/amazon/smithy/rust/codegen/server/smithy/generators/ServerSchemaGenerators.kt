/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext

/**
 * Renders a shape's schema statics next to its generated Rust type and exposes them as `Type::SCHEMA`.
 *
 * This mirrors the client's convention, so protocol code reaches a shape's schema through the type itself
 * rather than through a parallel module tree.
 */
class ServerSchemaConstantGenerator(
    private val codegenContext: ServerCodegenContext,
    private val writer: RustWriter,
    private val shape: Shape,
    private val additionalTraitInsertions: List<Writable> = emptyList(),
) {
    fun render() {
        ServerSchemaGenerator(
            codegenContext,
            writer,
            shape,
            additionalTraitInsertions = additionalTraitInsertions,
        ).renderSerializeOnly()
    }
}
