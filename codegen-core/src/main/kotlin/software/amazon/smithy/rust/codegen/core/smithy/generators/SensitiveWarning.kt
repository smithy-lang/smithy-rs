/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.traits.SensitiveTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.util.hasTrait

object SensitiveWarning {
    private const val warningMessage = "/// This data may contain sensitive information; It will not be obscured when serialized.\n"
    fun<T : Shape> addDoc(writer: RustWriter, shape: T) {
        if (shape.hasTrait<SensitiveTrait>()) {
            writer.writeInline(warningMessage)
        }
    }
}
