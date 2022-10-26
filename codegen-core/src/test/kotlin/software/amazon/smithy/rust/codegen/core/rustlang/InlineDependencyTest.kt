/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.rustlang

import io.kotest.matchers.shouldBe
import io.kotest.matchers.shouldNotBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest

internal class InlineDependencyTest {
    private fun makeDep(name: String) = InlineDependency(name, RustModule.private("module")) {
        rustBlock("fn foo()") {}
    }

    @Test
    fun `equal dependencies should be equal`() {
        val depA = makeDep("func")
        val depB = makeDep("func")
        depA.renderer shouldBe depB.renderer
        depA.key() shouldBe depB.key()

        depA.key() shouldNotBe makeDep("func2").key()
    }

    @Test
    fun `locate dependencies from the inlineable module`() {
        val dep = InlineDependency.idempotencyToken()
        val testWriter = RustWriter.root()
        testWriter.addDependency(CargoDependency.FastRand)
        testWriter.withModule(dep.module.copy(rustMetadata = RustMetadata(visibility = Visibility.PUBLIC))) {
            dep.renderer(this)
        }
        testWriter.compileAndTest(
            """
            use crate::idempotency_token::uuid_v4;
            let res = uuid_v4(0);
            assert_eq!(res, "00000000-0000-4000-8000-000000000000");
            """,
        )
    }
}
