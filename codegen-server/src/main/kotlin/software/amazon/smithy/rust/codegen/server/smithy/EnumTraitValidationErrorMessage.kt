/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.InternalTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait

fun EnumTrait.validationErrorMessage() =
    "Value at '{}' failed to satisfy constraint: Member must satisfy enum value set: [${enumValueSet()}]"

fun EnumTrait.shapeConstraintViolationDisplayMessage(shape: Shape) =
    "Value provided for '${shape.id}' failed to satisfy constraint: Member must satisfy enum value set: [${enumValueSet()}]"

fun EnumTrait.enumValueSet() = this.enumDefinitionValues.joinToString(", ")

fun EnumTrait.withoutInternalValues(shape: Shape): EnumTrait {
    val internalValues =
        if (shape is EnumShape) {
            shape.allMembers.values.filter { it.hasTrait<InternalTrait>() }.map { shape.enumValues.getValue(it.memberName) }
        } else {
            values.filter { it.hasTag("internal") }.map { it.value }
        }
    return toBuilder().apply { internalValues.forEach { removeEnum(it) } }.build()
}
