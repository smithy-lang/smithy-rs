/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package aws.sdk

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import java.io.File

private fun service(name: String): AwsService =
    AwsService(
        name,
        "test",
        "test",
        File("testmodel"),
        null,
        emptyList(),
        name,
    )

class IndependentCrateVersionerTest {
    @Test
    fun devPreviewSmithyRsChanged() {
        val dynamoDb = service("dynamodb")
        val ec2 = service("ec2")
        val s3 = service("s3")
        val someNewService = service("somenewservice")

        val versioner =
            IndependentCrateVersioner(
                VersionsManifest(
                    smithyRsRevision = "smithy-rs-1",
                    crates =
                        mapOf(
                            "aws-sdk-dynamodb" to
                                CrateVersion(
                                    category = "AwsSdk",
                                    version = "0.11.3",
                                    modelHash = "dynamodb-hash",
                                ),
                            "aws-sdk-ec2" to
                                CrateVersion(
                                    category = "AwsSdk",
                                    version = "0.10.1",
                                    modelHash = "ec2-hash",
                                ),
                            "aws-sdk-s3" to
                                CrateVersion(
                                    category = "AwsSdk",
                                    version = "0.12.0",
                                    modelHash = "s3-hash",
                                ),
                        ),
                ),
                ModelMetadata(
                    crates =
                        mapOf(
                            "aws-sdk-dynamodb" to ChangeType.FEATURE,
                            "aws-sdk-ec2" to ChangeType.DOCUMENTATION,
                        ),
                ),
                devPreview = true,
                smithyRsVersion = "smithy-rs-2",
                hashModelsFn = { service ->
                    when (service) {
                        dynamoDb -> "dynamodb-hash"
                        ec2 -> "ec2-hash"
                        s3 -> "s3-hash"
                        else -> throw IllegalStateException("unreachable")
                    }
                },
            )

        // The code generator changed, so all minor versions should bump
        assertEquals("0.12.0", versioner.decideCrateVersion("aws-sdk-dynamodb", dynamoDb))
        assertEquals("0.11.0", versioner.decideCrateVersion("aws-sdk-ec2", ec2))
        assertEquals("0.13.0", versioner.decideCrateVersion("aws-sdk-s3", s3))
        assertEquals("0.1.0", versioner.decideCrateVersion("aws-sdk-somenewservice", someNewService))
    }

    @Test
    fun devPreviewSameCodeGenerator() {
        val dynamoDb = service("dynamodb")
        val ec2 = service("ec2")
        val polly = service("polly")
        val s3 = service("s3")
        val someNewService = service("somenewservice")

        val versioner =
            IndependentCrateVersioner(
                VersionsManifest(
                    smithyRsRevision = "smithy-rs-1",
                    crates =
                        mapOf(
                            "aws-sdk-dynamodb" to
                                CrateVersion(
                                    category = "AwsSdk",
                                    version = "0.11.3",
                                    modelHash = "dynamodb-hash",
                                ),
                            "aws-sdk-ec2" to
                                CrateVersion(
                                    category = "AwsSdk",
                                    version = "0.10.1",
                                    modelHash = "ec2-hash",
                                ),
                            "aws-sdk-polly" to
                                CrateVersion(
                                    category = "AwsSdk",
                                    version = "0.9.0",
                                    modelHash = "old-polly-hash",
                                ),
                            "aws-sdk-s3" to
                                CrateVersion(
                                    category = "AwsSdk",
                                    version = "0.12.0",
                                    modelHash = "s3-hash",
                                ),
                        ),
                ),
                ModelMetadata(
                    crates =
                        mapOf(
                            "aws-sdk-dynamodb" to ChangeType.FEATURE,
                            "aws-sdk-ec2" to ChangeType.DOCUMENTATION,
                            // polly has a model change, but is absent from the model metadata file
                        ),
                ),
                devPreview = true,
                smithyRsVersion = "smithy-rs-1",
                hashModelsFn = { service ->
                    when (service) {
                        dynamoDb -> "dynamodb-hash"
                        ec2 -> "ec2-hash"
                        polly -> "NEW-polly-hash"
                        s3 -> "s3-hash"
                        else -> throw IllegalStateException("unreachable")
                    }
                },
            )

        assertEquals("0.11.4", versioner.decideCrateVersion("aws-sdk-dynamodb", dynamoDb))
        assertEquals("0.10.2", versioner.decideCrateVersion("aws-sdk-ec2", ec2))
        assertEquals("0.12.0", versioner.decideCrateVersion("aws-sdk-s3", s3))
        assertEquals("0.9.1", versioner.decideCrateVersion("aws-sdk-polly", polly))
        assertEquals("0.1.0", versioner.decideCrateVersion("aws-sdk-somenewservice", someNewService))
    }

    @Test
    fun smithyRsChanged() {
        val dynamoDb = service("dynamodb")
        val ec2 = service("ec2")
        val s3 = service("s3")
        val someNewService = service("somenewservice")

        val versioner =
            IndependentCrateVersioner(
                VersionsManifest(
                    smithyRsRevision = "smithy-rs-1",
                    crates =
                        mapOf(
                            "aws-sdk-dynamodb" to
                                CrateVersion(
                                    category = "AwsSdk",
                                    version = "1.11.3",
                                ),
                            "aws-sdk-ec2" to
                                CrateVersion(
                                    category = "AwsSdk",
                                    version = "1.10.1",
                                ),
                            "aws-sdk-s3" to
                                CrateVersion(
                                    category = "AwsSdk",
                                    version = "1.12.0",
                                ),
                        ),
                ),
                ModelMetadata(
                    crates =
                        mapOf(
                            "aws-sdk-dynamodb" to ChangeType.FEATURE,
                            "aws-sdk-ec2" to ChangeType.DOCUMENTATION,
                        ),
                ),
                devPreview = false,
                smithyRsVersion = "smithy-rs-2",
            )

        // The code generator changed, so all minor versions should bump
        assertEquals("1.12.0", versioner.decideCrateVersion("aws-sdk-dynamodb", dynamoDb))
        assertEquals("1.11.0", versioner.decideCrateVersion("aws-sdk-ec2", ec2))
        assertEquals("1.13.0", versioner.decideCrateVersion("aws-sdk-s3", s3))
        assertEquals("1.0.0", versioner.decideCrateVersion("aws-sdk-somenewservice", someNewService))
    }

    @Test
    fun sameCodeGenerator() {
        val dynamoDb = service("dynamodb")
        val ec2 = service("ec2")
        val polly = service("polly")
        val s3 = service("s3")
        val someNewService = service("somenewservice")

        val versioner =
            IndependentCrateVersioner(
                VersionsManifest(
                    smithyRsRevision = "smithy-rs-1",
                    crates =
                        mapOf(
                            "aws-sdk-dynamodb" to
                                CrateVersion(
                                    category = "AwsSdk",
                                    version = "1.11.3",
                                    modelHash = "dynamodb-hash",
                                ),
                            "aws-sdk-ec2" to
                                CrateVersion(
                                    category = "AwsSdk",
                                    version = "1.10.1",
                                    modelHash = "ec2-hash",
                                ),
                            "aws-sdk-polly" to
                                CrateVersion(
                                    category = "AwsSdk",
                                    version = "1.9.0",
                                    modelHash = "old-polly-hash",
                                ),
                            "aws-sdk-s3" to
                                CrateVersion(
                                    category = "AwsSdk",
                                    version = "1.12.0",
                                    modelHash = "s3-hash",
                                ),
                        ),
                ),
                ModelMetadata(
                    crates =
                        mapOf(
                            "aws-sdk-dynamodb" to ChangeType.FEATURE,
                            "aws-sdk-ec2" to ChangeType.DOCUMENTATION,
                            // polly has a model change, but is absent from the model metadata file
                        ),
                ),
                devPreview = false,
                smithyRsVersion = "smithy-rs-1",
                hashModelsFn = { service ->
                    when (service) {
                        dynamoDb -> "dynamodb-hash"
                        ec2 -> "ec2-hash"
                        polly -> "NEW-polly-hash"
                        s3 -> "s3-hash"
                        else -> throw IllegalStateException("unreachable")
                    }
                },
            )

        assertEquals("1.12.0", versioner.decideCrateVersion("aws-sdk-dynamodb", dynamoDb))
        assertEquals("1.10.2", versioner.decideCrateVersion("aws-sdk-ec2", ec2))
        assertEquals("1.10.0", versioner.decideCrateVersion("aws-sdk-polly", s3))
        assertEquals("1.12.0", versioner.decideCrateVersion("aws-sdk-s3", s3))
        assertEquals("1.0.0", versioner.decideCrateVersion("aws-sdk-somenewservice", someNewService))
    }
}

class HashModelsTest {
    @Test
    fun testHashModels() {
        val service =
            service("test").copy(
                modelFile = File("model1a"),
                extraFiles = listOf(File("model1b")),
            )
        val hash =
            hashModels(service) { file ->
                when (file.toString()) {
                    "model1a" -> "foo".toByteArray(Charsets.UTF_8)
                    "model1b" -> "bar".toByteArray(Charsets.UTF_8)
                    else -> throw IllegalStateException("unreachable")
                }
            }
        assertEquals("964021077fb6c3d42ae162ab2e2255be64c6d96a6d77bca089569774d54ef69b", hash)
    }
}
