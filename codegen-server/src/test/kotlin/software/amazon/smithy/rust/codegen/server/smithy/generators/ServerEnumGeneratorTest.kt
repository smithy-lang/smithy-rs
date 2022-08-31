/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import io.kotest.matchers.string.shouldNotContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProvider
import software.amazon.smithy.rust.codegen.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.util.expectTrait
import software.amazon.smithy.rust.codegen.util.lookup

class ServerEnumGeneratorTest {
    private val model = """
        namespace test
        enum InstanceType {
            @documentation("T2 instances are Burstable Performance Instances.")
            @tags(["ebsOnly"])
            T2_NANO = "t2.nano",

            @documentation("T2 instances are Burstable Performance Instances.")
            @tags(["ebsOnly"])
            T2_MICRO = "t2.micro",
        }
    """.asSmithyModel()

    @Test
    fun `it generates TryFrom, FromStr and errors for enums`() {
        val provider = serverTestSymbolProvider(model)
        val writer = RustWriter.forModule("model")
        val shape = model.lookup<StringShape>("test#InstanceType")
        val generator = ServerEnumGenerator(model, provider, writer, shape, shape.expectTrait(), TestRuntimeConfig)
        generator.render()
        writer.compileAndTest(
            """
            use std::str::FromStr;
            assert_eq!(InstanceType::try_from("t2.nano").unwrap(), InstanceType::T2Nano);
            assert_eq!(InstanceType::from_str("t2.nano").unwrap(), InstanceType::T2Nano);
            assert_eq!(InstanceType::try_from("unknown").unwrap_err(), InstanceTypeUnknownVariantError("unknown".to_string()));
            """,
        )
    }

    @Test
    fun `it generates enums without the unknown variant`() {
        val provider = serverTestSymbolProvider(model)
        val writer = RustWriter.forModule("model")
        val shape = model.lookup<StringShape>("test#InstanceType")
        val generator = ServerEnumGenerator(model, provider, writer, shape, shape.expectTrait(), TestRuntimeConfig)
        generator.render()
        writer.compileAndTest(
            """
            // check no unknown
            let instance = InstanceType::T2Micro;
            match instance {
                InstanceType::T2Micro => (),
                InstanceType::T2Nano => (),
            }
            """,
        )
    }

    @Test
    fun `it generates enums without non_exhaustive`() {
        val provider = serverTestSymbolProvider(model)
        val writer = RustWriter.forModule("model")
        val shape = model.lookup<StringShape>("test#InstanceType")
        val generator = ServerEnumGenerator(model, provider, writer, shape, shape.expectTrait(), TestRuntimeConfig)
        generator.render()
        writer.toString() shouldNotContain "#[non_exhaustive]"
    }
}
