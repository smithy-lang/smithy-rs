/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import io.kotest.assertions.throwables.shouldThrow
import io.kotest.matchers.collections.shouldContainExactly
import org.junit.jupiter.api.Test
import software.amazon.smithy.aws.traits.protocols.AwsJson1_1Trait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.protocol.traits.Rpcv2CborTrait
import software.amazon.smithy.rust.codegen.server.smithy.customize.ProtocolOrderConstraint

class ServerProtocolOrderTest {
    @Test
    fun `built-in order is deterministic regardless of loader order`() {
        ServerProtocolOrder.resolve(
            listOf(RestJson1Trait.ID, Rpcv2CborTrait.ID, AwsJson1_1Trait.ID),
            emptyList(),
        ).shouldContainExactly(Rpcv2CborTrait.ID, AwsJson1_1Trait.ID, RestJson1Trait.ID)
    }

    @Test
    fun `active decorator constraints order custom protocols`() {
        val custom = ShapeId.from("example.protocols#custom")
        ServerProtocolOrder.resolve(
            listOf(RestJson1Trait.ID, custom, Rpcv2CborTrait.ID),
            listOf(ProtocolOrderConstraint.Before(custom, Rpcv2CborTrait.ID)),
        ).shouldContainExactly(custom, Rpcv2CborTrait.ID, RestJson1Trait.ID)
    }

    @Test
    fun `inactive constraints do not affect selected protocols`() {
        val absent = ShapeId.from("example.protocols#absent")
        ServerProtocolOrder.resolve(
            listOf(RestJson1Trait.ID, Rpcv2CborTrait.ID),
            listOf(ProtocolOrderConstraint.Before(absent, RestJson1Trait.ID)),
        ).shouldContainExactly(Rpcv2CborTrait.ID, RestJson1Trait.ID)
    }

    @Test
    fun `cycles and duplicate selections fail fast`() {
        val first = ShapeId.from("example.protocols#first")
        val second = ShapeId.from("example.protocols#second")
        shouldThrow<CodegenException> {
            ServerProtocolOrder.resolve(
                listOf(first, second),
                listOf(
                    ProtocolOrderConstraint.Before(first, second),
                    ProtocolOrderConstraint.Before(second, first),
                ),
            )
        }
        shouldThrow<CodegenException> {
            ServerProtocolOrder.resolve(listOf(first, first), emptyList())
        }
    }
}
