/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.implBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule
import software.amazon.smithy.rust.codegen.server.smithy.customizations.SmithyValidationExceptionConversionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerRestJsonProtocol
import software.amazon.smithy.rust.codegen.server.smithy.renderInlineMemoryModules
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext

class ServerBuilderGeneratorTest {
    @Test
    fun `it respects the sensitive trait in Debug impl`() {
        val model = """
            namespace test
            @sensitive
            string SecretKey

            @sensitive
            string Password

            structure Credentials {
                username: String,
                password: Password,
                secretKey: SecretKey
            }
        """.asSmithyModel()

        val codegenContext = serverTestCodegenContext(model)
        val project = TestWorkspace.testProject()
        project.withModule(ServerRustModule.Model) {
            val writer = this
            val shape = model.lookup<StructureShape>("test#Credentials")

            StructureGenerator(model, codegenContext.symbolProvider, writer, shape, emptyList()).render()
            val builderGenerator = ServerBuilderGenerator(
                codegenContext,
                shape,
                SmithyValidationExceptionConversionGenerator(codegenContext),
                ServerRestJsonProtocol(codegenContext),
            )

            builderGenerator.render(project, writer)

            writer.implBlock(codegenContext.symbolProvider.toSymbol(shape)) {
                builderGenerator.renderConvenienceMethod(this)
            }

            project.renderInlineMemoryModules()
        }

        project.unitTest {
            rust(
                """
                use super::*;
                use crate::model::*;
                let builder = Credentials::builder()
                    .username(Some("admin".to_owned()))
                    .password(Some("pswd".to_owned()))
                    .secret_key(Some("12345".to_owned()));
                     assert_eq!(format!("{:?}", builder), "Builder { username: Some(\"admin\"), password: \"*** Sensitive Data Redacted ***\", secret_key: \"*** Sensitive Data Redacted ***\" }");
                """,
            )
        }
        project.compileAndTest()
    }
}
