/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.generators.protocol

import io.kotest.assertions.throwables.shouldThrow
import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.validation.ValidatedResultException
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

/**
 * Tests that Smithy's core `ShapeIdConflict` validator rejects any model that defines
 * two operation shape IDs differing only by case within the same namespace.
 *
 * This invariant guarantees that the RpcV2Cbor dual-route legacy alias (`{service}.{PascalCaseName}`)
 * can never collide with another operation's native key. If `getFoo` exists, then `GetFoo` cannot
 * exist in the same namespace, so PascalCasing `getFoo` into `GetFoo` is safe.
 *
 * See https://github.com/smithy-lang/smithy-rs/issues/4731
 */
class RpcV2CborShapeIdConflictInvariantTest {
    /**
     * Verify that a model with operations `getFoo` and `GetFoo` in the same namespace
     * fails to load due to ShapeIdConflict, BEFORE any codegen runs.
     */
    @Test
    fun `model with case-only operation name collision fails ShapeIdConflict validation`() {
        val exception =
            shouldThrow<ValidatedResultException> {
                """
                namespace test

                use smithy.protocols#rpcv2Cbor

                @rpcv2Cbor
                service Example {
                    operations: [getFoo, GetFoo],
                }

                /// Operation with camelCase name
                operation getFoo {
                    input:= { value: String }
                    output:= { result: String }
                }

                /// Operation with PascalCase name that differs only by case
                operation GetFoo {
                    input:= { data: String }
                    output:= { response: String }
                }
                """.asSmithyModel(smithyVersion = "2")
            }

        // The error must mention ShapeIdConflict - this is Smithy's built-in validator
        exception.message shouldContain "ShapeIdConflict"
    }
}
