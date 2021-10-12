/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.testutil.*
import software.amazon.smithy.rust.codegen.util.lookup

internal class EndpointConfigCustomizationTest {

    private val model = """
    namespace test
    @aws.api#service(sdkId: "Test", endpointPrefix: "service-with-prefix")
    service TestService {
        version: "123"
    }

    @aws.api#service(sdkId: "Test", endpointPrefix: "iam")
    service NoRegions {
        version: "123"
    }

    @aws.api#service(sdkId: "Test")
    service NoEndpointPrefix {
        version: "123"
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

    fun endpointCustomization(service: String) =
        EndpointConfigCustomization(
            testCodegenContext(
                model,
                model.lookup(service)
            ).copy(runtimeConfig = AwsTestRuntimeConfig),
            endpointConfig
        )

    @Test
    fun `generates valid code`() {
        validateConfigCustomizations(endpointCustomization("test#TestService"))
    }

    @Test
    fun `generates valid code when no endpoint prefix is provided`() {
        validateConfigCustomizations(endpointCustomization("test#NoEndpointPrefix"))
    }

    @Test
    fun `support region-specific endpoint overrides`() {
        val project =
            stubConfigProject(endpointCustomization("test#TestService"), TestWorkspace.testProject())
        project.lib {
            it.addDependency(awsTypes(AwsTestRuntimeConfig))
            it.addDependency(CargoDependency.Http)
            it.unitTest(
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
        project.compileAndTest()
    }

    @Test
    fun `support region-agnostic services`() {
        val project =
            stubConfigProject(endpointCustomization("test#NoRegions"), TestWorkspace.testProject())
        project.lib {
            it.addDependency(awsTypes(AwsTestRuntimeConfig))
            it.addDependency(CargoDependency.Http)
            it.unitTest(
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
        println("file:///" + project.baseDir + "/src/aws_endpoint.rs")
        project.compileAndTest()
    }
}
