/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.generators.config

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.customize.NamedSectionGenerator
import software.amazon.smithy.rust.codegen.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.testutil.unitTest
import software.amazon.smithy.rust.codegen.util.lookup

internal class ServiceConfigGeneratorTest {
    @Test
    fun `idempotency token when used`() {
        fun model(trait: String) = """
            namespace com.example

            use aws.protocols#restJson1
            use smithy.test#httpRequestTests
            use smithy.test#httpResponseTests

            @restJson1
            service HelloService {
                operations: [SayHello],
                version: "1"
            }

            operation SayHello {
                input: IdempotentInput
            }

            structure IdempotentInput {
                $trait
                tok: String
            }
        """.asSmithyModel()

        val withToken = model("@idempotencyToken")
        val withoutToken = model("")
        withToken.lookup<ServiceShape>("com.example#HelloService").needsIdempotencyToken(withToken) shouldBe true
        withoutToken.lookup<ServiceShape>("com.example#HelloService").needsIdempotencyToken(withoutToken) shouldBe false
    }

    @Test
    fun `find idempotency token via resources`() {
        val model = """
            namespace com.example
            service ResourceService {
                resources: [Resource],
                version: "1"
            }

            resource Resource {
                operations: [CreateResource]
            }
            operation CreateResource {
                input: IdempotentInput
            }

            structure IdempotentInput {
                @idempotencyToken
                tok: String
            }
        """.asSmithyModel()
        model.lookup<ServiceShape>("com.example#ResourceService").needsIdempotencyToken(model) shouldBe true
    }

    @Test
    fun `generate customizations as specified`() {
        class ServiceCustomizer : NamedSectionGenerator<ServiceConfig>() {
            override fun section(section: ServiceConfig): Writable {
                return when (section) {
                    ServiceConfig.ConfigStructAdditionalDocs -> emptySection
                    ServiceConfig.ConfigStruct -> writable { rust("config_field: u64,") }
                    ServiceConfig.ConfigImpl -> emptySection
                    ServiceConfig.BuilderStruct -> writable { rust("config_field: Option<u64>") }
                    ServiceConfig.BuilderImpl -> emptySection
                    ServiceConfig.BuilderBuild -> writable { rust("config_field: self.config_field.unwrap_or_default(),") }
                    else -> emptySection
                }
            }
        }
        val sut = ServiceConfigGenerator(listOf(ServiceCustomizer()))
        val symbolProvider = testSymbolProvider("namespace empty".asSmithyModel())
        val project = TestWorkspace.testProject(symbolProvider)
        project.withModule(RustModule.Config) {
            sut.render(it)
            it.unitTest(
                "set_config_fields",
                """
                let mut builder = Config::builder();
                builder.config_field = Some(99);
                let config = builder.build();
                assert_eq!(config.config_field, 99);
                """
            )
        }
        project.compileAndTest()
    }
}
