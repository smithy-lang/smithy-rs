/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.rustlang.DependencyScope.Compile

class CodegenDelegatorTest {
    @Test
    fun testMergeDependencyFeatures() {
        val merged = mergeDependencyFeatures(
            listOf(
                CargoDependency("A", CratesIo("1"), Compile, optional = false, features = setOf()),
                CargoDependency("A", CratesIo("1"), Compile, optional = false, features = setOf("f1")),
                CargoDependency("A", CratesIo("1"), Compile, optional = false, features = setOf("f2")),
                CargoDependency("A", CratesIo("1"), Compile, optional = false, features = setOf("f1", "f2")),

                CargoDependency("B", CratesIo("2"), Compile, optional = false, features = setOf()),
                CargoDependency("B", CratesIo("2"), Compile, optional = true, features = setOf()),

                CargoDependency("C", CratesIo("3"), Compile, optional = true, features = setOf()),
                CargoDependency("C", CratesIo("3"), Compile, optional = true, features = setOf()),
            ).shuffled()
        )

        merged shouldBe setOf(
            CargoDependency("A", CratesIo("1"), Compile, optional = false, features = setOf("f1", "f2")),
            CargoDependency("B", CratesIo("2"), Compile, optional = false, features = setOf()),
            CargoDependency("C", CratesIo("3"), Compile, optional = true, features = setOf()),
        )
    }
}
