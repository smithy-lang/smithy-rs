/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable

/** Abstraction for instantiating a builders.
 *
 * Builder abstractions vary—clients MAY use `build_with_error_correction`, e.g., and builders can vary in fallibility.
 * */
interface BuilderInstantiator {
    /** Set a field on a builder. */
    fun setField(builder: String, value: Writable, field: MemberShape): Writable

    /** Finalize a builder, turning into a built object (or in the case of builders-of-builders, return the builder directly).*/
    fun finalizeBuilder(builder: String, shape: StructureShape, mapErr: Writable? = null): Writable

    /** Set a field on a builder using the `$setterName` method. $value will be passed directly. */
    fun setFieldBase(builder: String, value: Writable, field: MemberShape) = writable {
        rustTemplate("$builder = $builder.${field.setterName()}(#{value})", "value" to value)
    }
}
