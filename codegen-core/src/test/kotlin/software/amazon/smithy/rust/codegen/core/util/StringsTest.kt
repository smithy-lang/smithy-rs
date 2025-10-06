/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.util

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.ExtensionContext
import org.junit.jupiter.api.fail
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.ArgumentsProvider
import org.junit.jupiter.params.provider.ArgumentsSource
import java.io.File
import java.util.stream.Stream

internal class StringsTest {
    @Test
    fun doubleQuote() {
        "abc".doubleQuote() shouldBe "\"abc\""
        """{"some": "json"}""".doubleQuote() shouldBe """"{\"some\": \"json\"}""""
        """{"nested": "{\"nested\": 5}"}"}""".doubleQuote() shouldBe
            """
            "{\"nested\": \"{\\\"nested\\\": 5}\"}\"}"
            """.trimIndent().trim()
    }

    @Test
    fun correctlyConvertToSnakeCase() {
        "NotificationARNs".toSnakeCase() shouldBe "notification_arns"
    }

    @Test
    fun handleDashes() {
        "application/x-amzn-json-1.1".toSnakeCase() shouldBe "application_x_amzn_json_1_1"
    }

    @Test
    fun testAllNames() {
        // Set this to true to write a new test expectation file
        val publishUpdate = false
        val allNames = this::class.java.getResource("/testOutput.txt")?.readText()!!
        val errors = mutableListOf<String>()
        val output = StringBuilder()
        allNames.lines().filter { it.isNotBlank() }.forEach {
            val split = it.split(',')
            val input = split[0]
            val expectation = split[1]
            val actual = input.toSnakeCase()
            if (input.toSnakeCase() != expectation) {
                errors += "$it => $actual (expected $expectation)"
            }
            output.appendLine("$input,$actual")
        }
        if (publishUpdate) {
            File("testOutput.txt").writeText(output.toString())
        }
        if (errors.isNotEmpty()) {
            fail(errors.joinToString("\n"))
        }
    }

    @ParameterizedTest
    @ArgumentsSource(TestCasesProvider::class)
    fun testSnakeCase(
        input: String,
        output: String,
    ) {
        input.toSnakeCase() shouldBe output
    }
}

class TestCasesProvider : ArgumentsProvider {
    override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
        listOf(
            "ACLs" to "acls",
            "ACLsUpdateStatus" to "acls_update_status",
            "AllowedAllVPCs" to "allowed_all_vpcs",
            "BluePrimaryX" to "blue_primary_x",
            "CIDRs" to "cidrs",
            "AuthTtL" to "auth_ttl",
            "CNAMEPrefix" to "cname_prefix",
            "S3Location" to "s3_location",
            "signatureS" to "signature_s",
            "signatureR" to "signature_r",
            "M3u8Settings" to "m3u8_settings",
            "IAMUser" to "iam_user",
            "OtaaV1_0_x" to "otaa_v1_0_x",
            "DynamoDBv2Action" to "dynamo_dbv2_action",
            "SessionKeyEmv2000" to "session_key_emv2000",
            "SupportsClassB" to "supports_class_b",
            "UnassignIpv6AddressesRequest" to "unassign_ipv6_addresses_request",
            "TotalGpuMemoryInMiB" to "total_gpu_memory_in_mib",
            "WriteIOs" to "write_ios",
            "dynamoDBv2" to "dynamo_dbv2",
            "ipv4Address" to "ipv4_address",
            "sigv4" to "sigv4",
            "s3key" to "s3_key",
            "sha256sum" to "sha256_sum",
            "Av1QvbrSettings" to "av1_qvbr_settings",
            "Av1Settings" to "av1_settings",
            "AwsElbv2LoadBalancer" to "aws_elbv2_load_balancer",
            "SigV4Authorization" to "sigv4_authorization",
            "IpV6Address" to "ipv6_address",
            "IpV6Cidr" to "ipv6_cidr",
            "IpV4Addresses" to "ipv4_addresses",
        ).map { Arguments.of(it.first, it.second) }.stream()
}
