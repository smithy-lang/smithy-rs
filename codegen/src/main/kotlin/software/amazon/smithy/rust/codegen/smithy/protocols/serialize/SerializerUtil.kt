/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.serialize

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.DoubleShape
import software.amazon.smithy.model.shapes.FloatShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rustBlock

class SerializerUtil(private val model: Model) {
    fun RustWriter.ignoreZeroValues(shape: MemberShape, value: ValueExpression, inner: RustWriter.() -> Unit) {
        val expr = when (model.expectShape(shape.target)) {
            is FloatShape, is DoubleShape -> "${value.asValue()} != 0.0"
            is NumberShape -> "${value.asValue()} != 0"
            is BooleanShape -> value.asValue()
            else -> null
        }

        if (expr == null ||
            // Required shapes should always be serialized
            // See https://github.com/awslabs/smithy-rs/issues/230 and https://github.com/aws/aws-sdk-go-v2/pull/1129
            shape.isRequired ||
            // Zero values are always serialized in lists and collections, this only applies to structures
            model.expectShape(shape.container) !is StructureShape
        ) {
            rustBlock("") {
                inner(this)
            }
        } else {
            rustBlock("if $expr") {
                inner(this)
            }
        }
    }
}
