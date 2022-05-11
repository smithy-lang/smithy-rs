/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.rustlang

import io.kotest.matchers.shouldBe
import io.kotest.matchers.shouldNotBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.testutil.compileAndTest

internal class InlineDependencyTest {
    fun makeDep(name: String) = InlineDependency(name, RustModule.private("module")) {
        it.rustBlock("fn foo()") {}
    }

    @Test
    fun `equal dependencies should be equal`() {
        val depa = makeDep("func")
        val depb = makeDep("func")
        depa.renderer shouldBe depb.renderer
        depa.key() shouldBe depb.key()

        depa.key() shouldNotBe makeDep("func2").key()
    }

    @Test
    fun `locate dependencies from the inlineable module`() {
        val dep = InlineDependency.idempotencyToken()
        val testWriter = RustWriter.root()
        testWriter.addDependency(CargoDependency.FastRand)
        testWriter.withModule(dep.module.name) {
            dep.renderer(this)
        }
        testWriter.compileAndTest(
            """
            use crate::idempotency_token::uuid_v4;
            let res = uuid_v4(0);
            assert_eq!(res, "00000000-0000-4000-8000-000000000000");
            """
        )
    }
}
