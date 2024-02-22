/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.server.smithy.generators.idWithHashEscaped

fun EnumTrait.validationErrorMessage() =
    "Value at '{}' failed to satisfy constraint: Member must satisfy enum value set: [${enumValueSet()}]"

fun EnumTrait.shapeConstraintViolationDisplayMessage(shape: Shape) =
    "Value provided for '${shape.idWithHashEscaped()}' failed to satisfy constraint: Member must satisfy enum value set: [${enumValueSet()}]"

fun EnumTrait.enumValueSet() = this.enumDefinitionValues.joinToString(", ")
