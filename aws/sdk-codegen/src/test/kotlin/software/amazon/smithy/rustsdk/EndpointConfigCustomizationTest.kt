/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.CodegenVisitor
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.customizations.AllowLintsGenerator
import software.amazon.smithy.rust.codegen.smithy.customize.CombinedCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.customize.RequiredCustomizations
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.generatePluginContext
import software.amazon.smithy.rust.codegen.testutil.stubConfigCustomization
import software.amazon.smithy.rust.codegen.testutil.unitTest
import software.amazon.smithy.rust.codegen.util.runCommand

internal class EndpointConfigCustomizationTest {

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
        @restJson1
        service NoRegions {
            version: "123",
            operations: [Nop]
        }

        @aws.api#service(sdkId: "Test")
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
        val (context, testDir) = generatePluginContext(model, service = service, runtimeConfig = AwsTestRuntimeConfig)
        val codegenDecorator = object : RustCodegenDecorator {
            override val name: String = "tests and config"
            override val order: Byte = 0
            override fun configCustomizations(
                codegenContext: CodegenContext,
                baseCustomizations: List<ConfigCustomization>
            ): List<ConfigCustomization> {
                return baseCustomizations + stubConfigCustomization("a") + EndpointConfigCustomization(
                    codegenContext,
                    endpointConfig
                ) + stubConfigCustomization("b")
            }

            override fun libRsCustomizations(
                codegenContext: CodegenContext,
                baseCustomizations: List<LibRsCustomization>
            ): List<LibRsCustomization> {
                return baseCustomizations + PubUseEndpoint(AwsTestRuntimeConfig) + AllowLintsGenerator(listOf("dead_code"), listOf(), listOf())
            }

            override fun extras(codegenContext: CodegenContext, rustCrate: RustCrate) {
                if (test != null) {
                    test(rustCrate)
                }
            }
        }
        val customization = CombinedCodegenDecorator(listOf(RequiredCustomizations(), codegenDecorator))
        CodegenVisitor(context, customization).execute()
        "cargo test".runCommand(testDir)
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
                it.addDependency(awsTypes(AwsTestRuntimeConfig))
                it.addDependency(CargoDependency.Http)
                it.unitTest(
                    "region_override",
                    """
                    use aws_types::region::Region;
                    use http::Uri;
                    let conf = crate::config::Config::builder().build();
                    let endpoint = conf.endpoint_resolver
                        .resolve_endpoint(&Region::new("fips-ca-central-1")).expect("default resolver produces a valid endpoint");
                    let mut uri = Uri::from_static("/?k=v");
                    endpoint.set_endpoint(&mut uri, None);
                    assert_eq!(uri, Uri::from_static("https://access-analyzer-fips.ca-central-1.amazonaws.com/?k=v"));
                    """
                )
            }
        }
    }

    @Test
    fun `support region-agnostic services`() {
        validateEndpointCustomizationForService("test#NoRegions") { crate ->
            crate.lib {
                it.addDependency(awsTypes(AwsTestRuntimeConfig))
                it.addDependency(CargoDependency.Http)
                it.unitTest(
                    "global_services",
                    """
                    use aws_types::region::Region;
                    use http::Uri;
                    let conf = crate::config::Config::builder().build();
                    let endpoint = conf.endpoint_resolver
                        .resolve_endpoint(&Region::new("us-east-1")).expect("default resolver produces a valid endpoint");
                    let mut uri = Uri::from_static("/?k=v");
                    endpoint.set_endpoint(&mut uri, None);
                    assert_eq!(uri, Uri::from_static("https://iam.amazonaws.com/?k=v"));

                    let endpoint = conf.endpoint_resolver
                        .resolve_endpoint(&Region::new("iam-fips")).expect("default resolver produces a valid endpoint");
                    let mut uri = Uri::from_static("/?k=v");
                    endpoint.set_endpoint(&mut uri, None);
                    assert_eq!(uri, Uri::from_static("https://iam-fips.amazonaws.com/?k=v"));
                    """
                )
            }
        }
    }
}
