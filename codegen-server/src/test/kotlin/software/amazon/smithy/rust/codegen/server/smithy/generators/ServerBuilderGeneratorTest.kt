/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.util.lookup
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
        val writer = RustWriter.forModule("model")
        val shape = model.lookup<StructureShape>("test#Credentials")
        StructureGenerator(model, codegenContext.symbolProvider, writer, shape).render(CodegenTarget.SERVER)
        val builderGenerator = ServerBuilderGenerator(codegenContext, shape)
        builderGenerator.render(writer)
        writer.implBlock(shape, codegenContext.symbolProvider) {
            builderGenerator.renderConvenienceMethod(this)
        }
        writer.compileAndTest(
            """
            use super::*;
            let builder = Credentials::builder()
                .username(Some("admin".to_owned()))
                .password(Some("pswd".to_owned()))
                .secret_key(Some("12345".to_owned()));
                 assert_eq!(format!("{:?}", builder), "Builder { username: Some(\"admin\"), password: \"*** Sensitive Data Redacted ***\", secret_key: \"*** Sensitive Data Redacted ***\" }");
            """,
        )
    }
}
