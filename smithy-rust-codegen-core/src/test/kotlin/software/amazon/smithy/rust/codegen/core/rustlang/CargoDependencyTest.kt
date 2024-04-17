/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.rustlang

import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows

class CargoDependencyTest {
    @Test
    fun `it should not allow a dependency with test-util in non-dev scopes`() {
        // OK
        CargoDependency(
            name = "test",
            location = CratesIo("1.0"),
            features = setOf("foo", "test-util", "bar"),
            scope = DependencyScope.Dev,
        )

        // OK
        CargoDependency(
            name = "test",
            location = CratesIo("1.0"),
            features = setOf("foo", "bar"),
            scope = DependencyScope.Dev,
        ).toDevDependency().withFeature("test-util")

        assertThrows<IllegalArgumentException> {
            CargoDependency(
                name = "test",
                location = CratesIo("1.0"),
                features = setOf("foo", "test-util", "bar"),
                scope = DependencyScope.Compile,
            )
        }

        assertThrows<IllegalArgumentException> {
            CargoDependency(
                name = "test",
                location = CratesIo("1.0"),
                features = setOf("foo", "bar"),
                scope = DependencyScope.Compile,
            ).withFeature("test-util")
        }
    }
}
