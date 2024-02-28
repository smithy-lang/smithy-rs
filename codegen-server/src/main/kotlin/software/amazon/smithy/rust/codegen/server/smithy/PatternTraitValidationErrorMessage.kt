/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.traits.PatternTrait

fun PatternTrait.validationErrorMessage() =
    "Value at '{}' failed to satisfy constraint: Member must satisfy regular expression pattern: {}"

fun PatternTrait.shapeConstraintViolationDisplayMessage(shape: Shape) =
    "Value provided for `${shape.id}` failed to satisfy the constraint: Member must match the regular expression pattern: {}"
