/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators.config

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.lang.Writable
import software.amazon.smithy.rust.codegen.lang.rust
import software.amazon.smithy.rust.codegen.lang.writeable
import software.amazon.smithy.rust.codegen.util.lookup
import software.amazon.smithy.rust.testutil.TestWorkspace
import software.amazon.smithy.rust.testutil.asSmithyModel
import software.amazon.smithy.rust.testutil.compileAndTest
import software.amazon.smithy.rust.testutil.testSymbolProvider
import software.amazon.smithy.rust.testutil.unitTest

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
    fun `generate customizations as specified`() {
        class ServiceCustomizer : NamedSectionGenerator<ServiceConfig>() {
            override fun section(section: ServiceConfig): Writable {
                return when (section) {
                    ServiceConfig.ConfigStruct -> writeable { rust("config_field: u64,") }
                    ServiceConfig.ConfigImpl -> emptySection
                    ServiceConfig.BuilderStruct -> writeable { rust("config_field: Option<u64>") }
                    ServiceConfig.BuilderImpl -> emptySection
                    ServiceConfig.BuilderBuild -> writeable { rust("config_field: self.config_field.unwrap_or_default(),") }
                }
            }
        }
        val sut = ServiceConfigGenerator(listOf(ServiceCustomizer()))
        val symbolProvider = testSymbolProvider("namespace empty".asSmithyModel())
        val project = TestWorkspace.testProject(symbolProvider)
        project.useFileWriter("src/config.rs", "crate::config") {
            sut.render(it)
            it.unitTest(
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
