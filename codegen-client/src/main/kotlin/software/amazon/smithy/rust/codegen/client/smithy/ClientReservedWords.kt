/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy

import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWordConfig
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator

val ClientReservedWords =
    RustReservedWordConfig(
        structureMemberMap =
            StructureGenerator.structureMemberNameMap +
                mapOf(
                    "send" to "send_value",
                    // To avoid conflicts with the `make_operation` and `presigned` functions on generated inputs
                    "make_operation" to "make_operation_value",
                    "presigned" to "presigned_value",
                    "customize" to "customize_value",
                    // To avoid conflicts with the error metadata `meta` field
                    "meta" to "meta_value",
                ),
        unionMemberMap =
            mapOf(
                // Unions contain an `Unknown` variant. This exists to support parsing data returned from the server
                // that represent union variants that have been added since this SDK was generated.
                UnionGenerator.UNKNOWN_VARIANT_NAME to "${UnionGenerator.UNKNOWN_VARIANT_NAME}Value",
                "${UnionGenerator.UNKNOWN_VARIANT_NAME}Value" to "${UnionGenerator.UNKNOWN_VARIANT_NAME}Value_",
            ),
        enumMemberMap =
            mapOf(
                // Unknown is used as the name of the variant containing unexpected values
                "Unknown" to "UnknownValue",
                // Real models won't end in `_` so it's safe to stop here
                "UnknownValue" to "UnknownValue_",
            ),
    )
