/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.server.smithy.isDirectlyConstrained

/**
 * Common helper functions used in [UnconstrainedMapGenerator] and [MapConstraintViolationGenerator].
 */

fun isKeyConstrained(shape: StringShape, symbolProvider: SymbolProvider) = shape.isDirectlyConstrained(symbolProvider)

fun isValueConstrained(shape: Shape, model: Model, symbolProvider: SymbolProvider): Boolean =
    shape.canReachConstrainedShape(model, symbolProvider)
