/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.matchers.shouldBe
import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.io.File

/**
 * Tests that verify protocol-specific module names are generated correctly.
 * Each protocol should generate a unique module name in the format: protocol_serde_{protocol_name}
 */
class ProtocolSpecificModuleTest {
    
    /**
     * Helper function to verify that generated code contains the expected protocol-specific module name.
     */
    private fun verifyProtocolModuleName(
        modelProtocol: ModelProtocol,
        expectedModuleName: String,
    ) {
        val (model, serviceShapeId) = loadSmithyConstraintsModelForProtocol(modelProtocol)
        
        val generatedServers = serverIntegrationTest(
            model,
            IntegrationTestParams(
                service = serviceShapeId.toString(),
            ),
        ) { _, _ ->
            // Simply compiling the crate is sufficient as a test
        }
        
        // Verify each generated server (there may be multiple for different HTTP versions)
        generatedServers.forEach { generatedServer ->
            val generatedCodeDir = generatedServer.path.toFile()
            val srcDir = File(generatedCodeDir, "src")
            srcDir.exists() shouldBe true
            
            // Check that the protocol-specific module directory exists
            val protocolModuleDir = File(srcDir, expectedModuleName)
            protocolModuleDir.exists() shouldBe true
            protocolModuleDir.isDirectory shouldBe true
            
            // Verify lib.rs declares the protocol-specific module
            val libRsFile = File(srcDir, "lib.rs")
            libRsFile.exists() shouldBe true
            val libRsContent = libRsFile.readText()
            libRsContent shouldContain "pub(crate) mod $expectedModuleName;"
        }
    }
    
    @Test
    fun `RestJson protocol generates protocol_serde_rest_json1 module`() {
        verifyProtocolModuleName(
            ModelProtocol.RestJson,
            "protocol_serde_rest_json1"
        )
    }
    
    @Test
    fun `Rpcv2Cbor protocol generates protocol_serde_rpcv2_cbor module`() {
        verifyProtocolModuleName(
            ModelProtocol.Rpcv2Cbor,
            "protocol_serde_rpcv2_cbor"
        )
    }
    
    @Test
    fun `RestXml protocol generates protocol_serde_rest_xml module`() {
        verifyProtocolModuleName(
            ModelProtocol.RestXml,
            "protocol_serde_rest_xml"
        )
    }
    
    @Test
    fun `AwsJson1_0 protocol generates protocol_serde_aws_json1_0 module`() {
        verifyProtocolModuleName(
            ModelProtocol.AwsJson10,
            "protocol_serde_aws_json1_0"
        )
    }
    
    @Test
    fun `AwsJson1_1 protocol generates protocol_serde_aws_json1_1 module`() {
        verifyProtocolModuleName(
            ModelProtocol.AwsJson11,
            "protocol_serde_aws_json1_1"
        )
    }
}
