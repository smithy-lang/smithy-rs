/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

/** Optional structured event serde strategy. Core owns framing, headers and primitive payloads. */
interface EventStreamSerdeCustomization {
    /** Owned context stored by generated marshallers and unmarshallers as `self.protocol`. */
    val contextType: RuntimeType

    /** Expression producing payload bytes from a completed modeled value. */
    fun serialize(
        target: Shape,
        value: String,
    ): Writable

    /** Expression reading `message.payload()` into a completed value, including validation. */
    fun deserialize(
        target: Shape,
        exception: Boolean,
    ): Writable

    /**
     * Borrowed media-type expression, evaluated before dispatch even for primitive or empty events.
     * Context validation failures must use the requested runtime error direction.
     */
    fun mediaType(unmarshalling: Boolean = false): Writable
}
