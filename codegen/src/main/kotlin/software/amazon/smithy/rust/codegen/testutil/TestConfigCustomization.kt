/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.testutil

import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfigGenerator

fun stubCustomization(name: String): ConfigCustomization {
    return object : ConfigCustomization() {
        override fun section(section: ServiceConfig): Writable = writable {
            when (section) {
                ServiceConfig.ConfigStruct -> rust("$name: u64,")
                ServiceConfig.ConfigImpl -> emptySection
                ServiceConfig.BuilderStruct -> rust("$name: Option<u64>,")
                ServiceConfig.BuilderImpl -> rust(
                    """
                    pub fn $name(mut self, $name: u64) -> Self {
                            self.$name = Some($name);
                        self
                    }
                """
                )
                ServiceConfig.BuilderBuild -> rust(
                    """
                    $name: self.$name.unwrap_or(123),
                """
                )
            }
        }
    }
}

/** Basic validation of [ConfigCustomization]s
 *
 * This test is not comprehensive, but it ensures that your customization generates Rust code that compiles and correctly
 * composes with other customizations.
 * */
fun validateConfigCustomizations(vararg customization: ConfigCustomization) {
    val customizations = listOf(stubCustomization("a")) + customization.toList() + stubCustomization("b")
    val generator = ServiceConfigGenerator(customizations = customizations.toList())
    val project = TestWorkspace.testProject()
    project.useFileWriter("src/config.rs", "crate::config") {
        generator.render(it)
    }
    project.compileAndTest()
}
