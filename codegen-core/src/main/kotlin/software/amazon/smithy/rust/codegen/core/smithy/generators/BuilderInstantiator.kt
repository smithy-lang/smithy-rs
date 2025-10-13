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

/** Abstraction for instantiating builders.
 *
 * Builder abstractions vary—clients MAY use `build_with_error_correction`, e.g., and builders can vary in fallibility.
 * */
interface BuilderInstantiator {
    /** Set a field on a builder. */
    fun setField(
        builder: String,
        value: Writable,
        field: MemberShape,
    ): Writable

    /** Finalize a builder, turning it into a built object
     *  - In the case of builders-of-builders, the value should be returned directly
     *  - If an error is returned, you MUST use `mapErr` to convert the error type
     */
    fun finalizeBuilder(
        builder: String,
        shape: StructureShape,
        mapErr: Writable? = null,
    ): Writable

    fun setterProvider(field: MemberShape): String

    /** Set a field on a builder using the `$setterName` method. $value will be passed directly. */
    fun setFieldWithSetter(
        builder: String,
        value: Writable,
        field: MemberShape,
    ) = writable {
        rustTemplate("$builder = $builder.${setterProvider(field)}(#{value})", "value" to value)
    }
}
