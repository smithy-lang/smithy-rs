/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.util

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.util.deepMergeWith

class MapTest {
    @Test
    fun `it should deep merge maps`() {
        mapOf<String, Any?>().deepMergeWith(mapOf()) shouldBe emptyMap()

        mapOf<String, Any?>("foo" to 1, "bar" to "baz").deepMergeWith(mapOf()) shouldBe mapOf(
            "foo" to 1,
            "bar" to "baz"
        )

        mapOf<String, Any?>().deepMergeWith(mapOf("foo" to 1, "bar" to "baz")) shouldBe mapOf(
            "foo" to 1,
            "bar" to "baz"
        )

        mapOf<String, Any?>(
            "package" to mapOf<String, Any?>(
                "name" to "foo",
                "version" to "1.0.0",
            )
        ).deepMergeWith(
            mapOf<String, Any?>(
                "package" to mapOf<String, Any?>(
                    "readme" to "README.md",
                )
            )
        ) shouldBe mapOf(
            "package" to mapOf<String, Any?>(
                "name" to "foo",
                "version" to "1.0.0",
                "readme" to "README.md",
            )
        )

        mapOf<String, Any?>(
            "package" to mapOf<String, Any?>(
                "name" to "foo",
                "version" to "1.0.0",
                "overwrite-me" to "wrong",
            ),
            "make-me-not-a-map" to mapOf("foo" to "bar"),
        ).deepMergeWith(
            mapOf<String, Any?>(
                "package" to mapOf<String, Any?>(
                    "readme" to "README.md",
                    "overwrite-me" to "correct",
                ),
                "make-me-not-a-map" to 5
            )
        ) shouldBe mapOf(
            "package" to mapOf<String, Any?>(
                "name" to "foo",
                "version" to "1.0.0",
                "readme" to "README.md",
                "overwrite-me" to "correct",
            ),
            "make-me-not-a-map" to 5
        )
    }
}
