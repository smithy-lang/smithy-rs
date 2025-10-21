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
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderInstantiator
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName

class ClientBuilderInstantiator(private val clientCodegenContext: ClientCodegenContext) : BuilderInstantiator {
    override fun setField(
        builder: String,
        value: Writable,
        field: MemberShape,
    ): Writable {
        return setFieldWithSetter(builder, value, field)
    }

    override fun setterProvider(field: MemberShape): String {
        return field.setterName(clientCodegenContext.symbolProvider)
    }

    /**
     * For the client, we finalize builders with error correction enabled
     */
    override fun finalizeBuilder(
        builder: String,
        shape: StructureShape,
        mapErr: Writable?,
    ): Writable =
        writable {
            val correctErrors = clientCodegenContext.correctErrors(shape)
            val builderW =
                writable {
                    when {
                        correctErrors != null -> rustTemplate("#{correctErrors}($builder)", "correctErrors" to correctErrors)
                        else -> rustTemplate(builder)
                    }
                }
            if (BuilderGenerator.hasFallibleBuilder(shape, clientCodegenContext.symbolProvider)) {
                rustTemplate(
                    "#{builder}.build()#{mapErr}",
                    "builder" to builderW,
                    "mapErr" to (
                        mapErr?.map {
                            rust(".map_err(#T)?", it)
                        } ?: writable { }
                    ),
                )
            } else {
                rustTemplate(
                    "#{builder}.build()",
                    "builder" to builderW,
                )
            }
        }
}
