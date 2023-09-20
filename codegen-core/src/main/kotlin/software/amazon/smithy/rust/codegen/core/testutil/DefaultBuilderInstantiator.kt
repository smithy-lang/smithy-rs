/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.testutil

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderInstantiator

/**
 * A Default instantiator that uses `builder.build()` in all cases. This exists to support tests in codegen-core
 * and to serve as the base behavior for client and server instantiators.
 */
class DefaultBuilderInstantiator : BuilderInstantiator {
    override fun setField(builder: String, value: Writable, field: MemberShape): Writable {
        return setFieldWithSetter(builder, value, field)
    }

    override fun finalizeBuilder(builder: String, shape: StructureShape, mapErr: Writable?): Writable {
        return writable { rust("builder.build()") }
    }
}
