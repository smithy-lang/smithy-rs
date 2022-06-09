/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.testutil

import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfigGenerator

/**
 * Test helper to produce a valid config customization to test that a [ConfigCustomization] can be used in conjunction
 * with other [ConfigCustomization]s.
 */
fun stubConfigCustomization(name: String): ConfigCustomization {
    return object : ConfigCustomization() {
        override fun section(section: ServiceConfig): Writable = writable {
            when (section) {
                ServiceConfig.ConfigStruct -> rust("_$name: u64,")
                ServiceConfig.ConfigImpl -> emptySection
                ServiceConfig.BuilderStruct -> rust("_$name: Option<u64>,")
                ServiceConfig.BuilderImpl -> rust(
                    """
                    /// docs!
                    pub fn $name(mut self, $name: u64) -> Self {
                            self._$name = Some($name);
                        self
                    }
                    """
                )
                ServiceConfig.BuilderBuild -> rust(
                    """
                    _$name: self._$name.unwrap_or(123),
                    """
                )
                else -> emptySection
            }
        }
    }
}

/** Basic validation of [ConfigCustomization]s
 *
 * This test is not comprehensive, but it ensures that your customization generates Rust code that compiles and correctly
 * composes with other customizations.
 * */
@Suppress("NAME_SHADOWING")
fun validateConfigCustomizations(
    customization: ConfigCustomization,
    project: TestWriterDelegator? = null
): TestWriterDelegator {
    val project = project ?: TestWorkspace.testProject()
    stubConfigProject(customization, project)
    project.compileAndTest()
    return project
}

fun stubConfigProject(customization: ConfigCustomization, project: TestWriterDelegator): TestWriterDelegator {
    val customizations = listOf(stubConfigCustomization("a")) + customization + stubConfigCustomization("b")
    val generator = ServiceConfigGenerator(customizations = customizations.toList())
    project.withModule(RustModule.Config) {
        generator.render(it)
        it.unitTest(
            "config_send_sync",
            """
            fn assert_send_sync<T: Send + Sync>() {}
            assert_send_sync::<Config>();
            """
        )
    }
    project.lib { it.rust("pub use config::Config;") }
    return project
}
