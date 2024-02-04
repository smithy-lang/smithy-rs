/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.traits.DefaultTrait
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.core.util.shapeId

internal class RemoveDefaultsTest {
    @Test
    fun `defaults should be removed`() {
        val removeDefaults =
            setOf(
                "test#Bar".shapeId(),
                "test#Foo\$baz".shapeId(),
            )
        val baseModel =
            """
            namespace test

            structure Foo {
                bar: Bar = 0
                baz: Integer = 0
            }

            @default(0)
            integer Bar

            """.asSmithyModel(smithyVersion = "2.0")
        val model = RemoveDefaults.processModel(baseModel, removeDefaults)
        val barMember = model.lookup<MemberShape>("test#Foo\$bar")
        barMember.hasTrait<DefaultTrait>() shouldBe false
        val bazMember = model.lookup<MemberShape>("test#Foo\$baz")
        bazMember.hasTrait<DefaultTrait>() shouldBe false
        val root = model.lookup<IntegerShape>("test#Bar")
        root.hasTrait<DefaultTrait>() shouldBe false
    }
}
