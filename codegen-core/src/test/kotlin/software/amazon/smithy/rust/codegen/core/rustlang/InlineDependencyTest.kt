/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.rustlang

import io.kotest.assertions.throwables.shouldThrow
import io.kotest.matchers.shouldBe
import io.kotest.matchers.shouldNotBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import kotlin.io.path.pathString

internal class InlineDependencyTest {
    private fun makeDep(name: String) =
        InlineDependency(name, RustModule.private("module")) {
            rustBlock("fn foo()") {}
        }

    @Test
    fun `equal dependencies should be equal`() {
        val depA = makeDep("func")
        val depB = makeDep("func")
        depA.renderer shouldNotBe depB.renderer
        depA.key() shouldBe depB.key()

        depA.key() shouldNotBe makeDep("func2").key()
    }

    @Test
    fun `locate dependencies from the inlineable module`() {
        val runtimeConfig = TestRuntimeConfig
        val dep = InlineDependency.serializationSettings(runtimeConfig)
        val testProject = TestWorkspace.testProject()
        testProject.lib {
            rustTemplate(
                """

                ##[test]
                fn header_serialization_settings_can_be_constructed() {
                    use #{serialization_settings}::HeaderSerializationSettings;
                    use #{aws_smithy_http}::header::set_request_header_if_absent;
                    let _settings = HeaderSerializationSettings::default();
                }

                """,
                "serialization_settings" to dep.toType(),
                "aws_smithy_http" to RuntimeType.smithyHttp(runtimeConfig),
            )
        }
        testProject.compileAndTest()
    }

    @Test
    fun `nested dependency modules`() {
        val a = RustModule.public("a")
        val b = RustModule.public("b", parent = a)
        val c = RustModule.public("c", parent = b)
        val type =
            RuntimeType.forInlineFun("forty2", c) {
                rust(
                    """
                    pub fn forty2() -> usize { 42 }
                    """,
                )
            }
        val crate = TestWorkspace.testProject()
        crate.lib {
            unitTest("use_nested_module") {
                rustTemplate("assert_eq!(42, #{forty2}())", "forty2" to type)
            }
        }
        crate.compileAndTest()
        val generatedFiles = crate.generatedFiles().map { it.pathString }
        assert(generatedFiles.contains("src/a.rs")) { generatedFiles }
        assert(generatedFiles.contains("src/a/b.rs")) { generatedFiles }
        assert(generatedFiles.contains("src/a/b/c.rs")) { generatedFiles }
    }

    @Test
    fun `prevent the creation of duplicate modules`() {
        val root = RustModule.private("parent")
        // create a child module with no docs
        val child1 = RustModule.public("child", parent = root)
        val child2 = RustModule.private("child", parent = root)
        val crate = TestWorkspace.testProject()
        crate.withModule(child1) { }
        shouldThrow<IllegalStateException> {
            crate.withModule(child2) {}
        }

        shouldThrow<IllegalStateException> {
            // can't make one with docs when the old one had no docs
            crate.withModule(child1.copy(documentationOverride = "docs")) {}
        }

        // but making an identical module is fine
        val identicalChild = RustModule.public("child", parent = root)
        crate.withModule(identicalChild) {}
        identicalChild.fullyQualifiedPath() shouldBe "crate::parent::child"
    }
}
