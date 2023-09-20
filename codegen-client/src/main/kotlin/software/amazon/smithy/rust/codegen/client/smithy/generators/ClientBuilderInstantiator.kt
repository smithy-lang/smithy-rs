/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.map
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderInstantiator

fun ClientCodegenContext.builderInstantiator(): BuilderInstantiator = ClientBuilderInstantiator(symbolProvider)

class ClientBuilderInstantiator(private val symbolProvider: RustSymbolProvider) : BuilderInstantiator {
    override fun setField(builder: String, value: Writable, field: MemberShape): Writable {
        return setFieldWithSetter(builder, value, field)
    }

    override fun finalizeBuilder(builder: String, shape: StructureShape, mapErr: Writable?): Writable = writable {
        if (BuilderGenerator.hasFallibleBuilder(shape, symbolProvider)) {
            rustTemplate(
                "$builder.build()#{mapErr}?",
                "mapErr" to (
                    mapErr?.map {
                        rust(".map_err(#T)", it)
                    } ?: writable { }
                    ),
            )
        } else {
            rust("$builder.build()")
        }
    }
}
