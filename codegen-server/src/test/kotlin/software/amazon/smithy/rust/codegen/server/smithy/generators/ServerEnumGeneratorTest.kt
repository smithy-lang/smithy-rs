/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import io.kotest.matchers.string.shouldNotContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.customizations.SmithyValidationExceptionConversionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext

class ServerEnumGeneratorTest {
    private val model =
        """
        namespace test
        @enum([
            {
                value: "t2.nano",
                name: "T2_NANO",
                documentation: "T2 instances are Burstable Performance Instances.",
                tags: ["ebsOnly"]
            },
            {
                value: "t2.micro",
                name: "T2_MICRO",
                documentation: "T2 instances are Burstable Performance Instances.",
                tags: ["ebsOnly"]
            },
        ])
        string InstanceType
        """.asSmithyModel()

    private val codegenContext = serverTestCodegenContext(model)
    private val writer = RustWriter.forModule("model")
    private val shape = model.lookup<StringShape>("test#InstanceType")

    @Test
    fun `it generates TryFrom, FromStr and errors for enums`() {
        ServerEnumGenerator(
            codegenContext,
            shape,
            SmithyValidationExceptionConversionGenerator(codegenContext),
            emptyList(),
        ).render(writer)
        writer.compileAndTest(
            """
            use std::str::FromStr;
            assert_eq!(InstanceType::try_from("t2.nano").unwrap(), InstanceType::T2Nano);
            assert_eq!(InstanceType::from_str("t2.nano").unwrap(), InstanceType::T2Nano);
            assert_eq!(InstanceType::try_from("unknown").unwrap_err(), crate::model::instance_type::ConstraintViolation(String::from("unknown")));
            """,
        )
    }

    @Test
    fun `it generates enums without the unknown variant`() {
        ServerEnumGenerator(
            codegenContext,
            shape,
            SmithyValidationExceptionConversionGenerator(codegenContext),
            emptyList(),
        ).render(writer)
        writer.compileAndTest(
            """
            // Check no `Unknown` variant.
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
        ServerEnumGenerator(
            codegenContext,
            shape,
            SmithyValidationExceptionConversionGenerator(codegenContext),
            emptyList(),
        ).render(writer)
        writer.toString() shouldNotContain "#[non_exhaustive]"
    }
}
