package software.amazon.smithy.rust.codegen.lang

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.testutil.compileAndTest

internal class InlineDependencyTest {
    fun makeDep() = InlineDependency("func", "module") {
        it.rustBlock("fn foo()") {}
    }

    @Test
    fun `equal dependencies should be equal`() {
        val depa = makeDep()
        val depb = makeDep()
        depa.renderer shouldBe depb.renderer
    }

    @Test
    fun `locate dependencies from the inlineable module`() {
        val dep = InlineDependency.uuid()
        val testWriter = RustWriter.forModule(null)
        testWriter.withModule("uuid") {
            dep.renderer(this)
        }
        testWriter.compileAndTest(
            """
            use crate::uuid;
            let res = uuid::v4(0);
            assert_eq!(res, "00000000-0000-4000-8000-000000000000");
        """
        )
    }
}
