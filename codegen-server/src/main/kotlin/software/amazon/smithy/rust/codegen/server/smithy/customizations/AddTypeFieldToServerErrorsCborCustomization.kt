/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.CborSerializerCustomization
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.CborSerializerSection
import software.amazon.smithy.rust.codegen.core.util.hasTrait

/**
 * Smithy RPC v2 CBOR requires errors to be serialized in server responses with an additional `__type` field.
 *
 * Note that we apply this customization when serializing _any_ structure with the `@error` trait, regardless if it's
 * an error response or not. Consider this model:
 *
 * ```smithy
 * operation ErrorSerializationOperation {
 *     input: SimpleStruct
 *     output: ErrorSerializationOperationOutput
 *     errors: [ValidationException]
 * }
 *
 * structure ErrorSerializationOperationOutput {
 *     errorShape: ValidationException
 * }
 * ```
 *
 * `ValidationException` is re-used across the operation output and the operation error. The `__type` field will
 * appear when serializing both.
 *
 * Strictly speaking, the spec says we should only add `__type` when serializing an operation error response, but
 * there shouldn't™️ be any harm in always including it, which simplifies the code generator.
 */
class AddTypeFieldToServerErrorsCborCustomization : CborSerializerCustomization() {
    override fun section(section: CborSerializerSection): Writable =
        when (section) {
            is CborSerializerSection.BeforeSerializingStructureMembers ->
                if (section.structContext.shape.hasTrait<ErrorTrait>()) {
                    writable {
                        rust(
                            """
                            ${section.encoderBindingName}
                                .str("__type")
                                .str("${escape(section.structContext.shape.id.toString())}");
                            """,
                        )
                    }
                } else {
                    emptySection
                }
            else -> emptySection
        }
}
