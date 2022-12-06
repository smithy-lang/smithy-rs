/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import java.io.File

internal class EndpointConfigCustomizationTest {
    private val placeholderEndpointParams = AwsRuntimeType.awsEndpoint(AwsTestRuntimeConfig).resolve("Params")
    private val codegenScope = arrayOf(
        "http" to RuntimeType.Http,
        "PlaceholderParams" to placeholderEndpointParams,
        "aws_types" to AwsRuntimeType.awsTypes(AwsTestRuntimeConfig),
    )

    private val model = """
        namespace test
        use aws.protocols#restJson1

        @title("test")
        @restJson1
        @aws.api#service(sdkId: "Test", endpointPrefix: "service-with-prefix")
        service TestService {
            version: "123",
            operations: [Nop]
        }

        @http(uri: "/foo", method: "GET")
        operation Nop {
        }

        @aws.api#service(sdkId: "Test", endpointPrefix: "iam")
        @title("test")
        @restJson1
        service NoRegions {
            version: "123",
            operations: [Nop]
        }

        @aws.api#service(sdkId: "Test")
        @title("test")
        @restJson1
        service NoEndpointPrefix {
            version: "123",
            operations: [Nop]
        }
    """.asSmithyModel()

    private val endpointConfig = """
        {
          "partitions" : [ {
            "defaults" : {
              "hostname" : "{service}.{region}.{dnsSuffix}",
              "protocols" : [ "https" ],
              "signatureVersions" : [ "v4" ]
            },
            "dnsSuffix" : "amazonaws.com",
            "partition" : "aws",
            "partitionName" : "AWS Standard",
            "regionRegex" : "^(us|eu|ap|sa|ca|me|af)\\-\\w+\\-\\d+${'$'}",
            "regions" : {
              "af-south-1" : {
                "description" : "Africa (Cape Town)"
              },
              "us-west-2" : {
                "description" : "US West (Oregon)"
              }
            },
            "services" : {
              "service-with-prefix" : {
                "endpoints" : {
                  "fips-ca-central-1" : {
                    "credentialScope" : {
                      "region" : "ca-central-1"
                    },
                    "hostname" : "access-analyzer-fips.ca-central-1.amazonaws.com"
                  },
                  "fips-us-west-1" : {
                    "credentialScope" : {
                      "region" : "us-west-1"
                    },
                    "hostname" : "access-analyzer-fips.us-west-1.amazonaws.com"
                  }
                }
              },
              "iam" : {
                "endpoints" : {
                  "aws-global" : {
                    "credentialScope" : {
                      "region" : "us-east-1"
                    },
                    "hostname" : "iam.amazonaws.com"
                  },
                  "iam-fips" : {
                    "credentialScope" : {
                      "region" : "us-east-1"
                    },
                    "hostname" : "iam-fips.amazonaws.com"
                  }
                },
                "isRegionalized" : false,
                "partitionEndpoint" : "aws-global"
              }
            }
        }]
        }
    """.let { ObjectNode.parse(it).expectObjectNode() }

    private fun validateEndpointCustomizationForService(service: String, test: ((RustCrate) -> Unit)? = null) {
        val endpointsFile = File.createTempFile("endpoints", ".json")
        endpointsFile.writeText(Node.printJson(endpointConfig))
        clientIntegrationTest(
            model,
            listOf(),
            service = service,
            runtimeConfig = AwsTestRuntimeConfig,
            additionalSettings = ObjectNode.builder()
                .withMember(
                    "customizationConfig",
                    ObjectNode.builder()
                        .withMember(
                            "awsSdk",
                            ObjectNode.builder()
                                .withMember("integrationTestPath", "../sdk/integration-tests")
                                .withMember("endpointsConfigPath", endpointsFile.absolutePath)
                                .build(),
                        ).build(),
                )
                .withMember("codegen", ObjectNode.builder().withMember("includeFluentClient", false).build()).build(),
        ) { _, rustCrate ->
            if (test != null) {
                test(rustCrate)
            }
        }
    }

    @Test
    fun `generates valid code`() {
        validateEndpointCustomizationForService("test#TestService")
    }

    @Test
    fun `generates valid code when no endpoint prefix is provided`() {
        validateEndpointCustomizationForService("test#NoEndpointPrefix")
    }

    @Test
    fun `support region-specific endpoint overrides`() {
        validateEndpointCustomizationForService("test#TestService") { crate ->
            crate.lib {
                unitTest("region_override") {
                    rustTemplate(
                        """
                        let conf = crate::config::Config::builder().build();
                        let endpoint = conf.endpoint_resolver
                            .resolve_endpoint(&::#{PlaceholderParams}::new(Some(#{aws_types}::region::Region::new("fips-ca-central-1")))).expect("default resolver produces a valid endpoint");
                        assert_eq!(endpoint.url(), "https://access-analyzer-fips.ca-central-1.amazonaws.com/");
                        """,
                        *codegenScope,
                    )
                }
            }
        }
    }

    @Test
    fun `support region-agnostic services`() {
        validateEndpointCustomizationForService("test#NoRegions") { crate ->
            crate.lib {
                unitTest("global_services") {
                    rustTemplate(
                        """
                        let conf = crate::config::Config::builder().build();
                        let endpoint = conf.endpoint_resolver
                            .resolve_endpoint(&::#{PlaceholderParams}::new(Some(#{aws_types}::region::Region::new("us-east-1")))).expect("default resolver produces a valid endpoint");
                        assert_eq!(endpoint.url(), "https://iam.amazonaws.com/");

                        let endpoint = conf.endpoint_resolver
                            .resolve_endpoint(&::#{PlaceholderParams}::new(Some(#{aws_types}::region::Region::new("iam-fips")))).expect("default resolver produces a valid endpoint");
                        assert_eq!(endpoint.url(), "https://iam-fips.amazonaws.com/");
                        """,
                        *codegenScope,
                    )
                }
            }
        }
    }
}
