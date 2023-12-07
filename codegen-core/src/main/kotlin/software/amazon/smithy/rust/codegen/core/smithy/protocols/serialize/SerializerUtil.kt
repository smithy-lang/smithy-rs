/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock

class SerializerUtil(private val model: Model) {
    fun RustWriter.ignoreZeroValues(shape: MemberShape, value: ValueExpression, inner: Writable) {
        // Required shapes should always be serialized
        // See https://github.com/smithy-lang/smithy-rs/issues/230 and https://github.com/aws/aws-sdk-go-v2/pull/1129
        if (
            shape.isRequired ||
            // Zero values are always serialized in lists and collections, this only applies to structures
            model.expectShape(shape.container) !is StructureShape
        ) {
            rustBlock("") {
                inner(this)
            }
        } else {
            this.ifNotDefault(model.expectShape(shape.target), value) { inner(this) }
        }
    }
}
