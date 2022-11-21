/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.rustlang

import io.kotest.assertions.throwables.shouldThrow
import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test

internal class RustModuleTest {
    @Test
    fun `prevent the creation of duplicate modules`() {
        val root = RustModule.private("parent")
        // create a child module with no docs
        RustModule.private("child", parent = root)
        shouldThrow<IllegalStateException> {
            // can't make a public module when we made a private one before
            RustModule.public("child", parent = root)
        }

        shouldThrow<IllegalStateException> {
            // can't make one with docs when the old one had no docs
            RustModule.private("child", documentation = "docs", parent = root)
        }

        // but making an identical module is fine
        val identicalChild = RustModule.private("child", parent = root)
        identicalChild.fullyQualifiedPath() shouldBe "crate::parent::child"
    }
}
