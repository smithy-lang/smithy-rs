package software.amazon.smithy.rust.codegen.lang

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test

internal class InlineDependencyTest {
    fun makeDep() = InlineDependency("func", "module") {
        it.rustBlock("fn foo()") {}
    }
    @Test
    fun `dependency equality`() {
        val depa = makeDep()
        val depb = makeDep()
        depa.renderer shouldBe depb.renderer
    }
}
