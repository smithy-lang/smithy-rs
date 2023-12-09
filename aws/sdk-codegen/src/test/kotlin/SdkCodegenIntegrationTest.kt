/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rustsdk.awsIntegrationTestParams
import software.amazon.smithy.rustsdk.awsSdkIntegrationTest

class SdkCodegenIntegrationTest {
    companion object {
        val model = """
            namespace test

            use aws.api#service
            use aws.auth#sigv4
            use aws.protocols#restJson1
            use smithy.rules#endpointRuleSet

            @service(sdkId: "dontcare")
            @restJson1
            @sigv4(name: "dontcare")
            @auth([sigv4])
            @endpointRuleSet({
                "version": "1.0",
                "rules": [{ "type": "endpoint", "conditions": [], "endpoint": { "url": "https://example.com" } }],
                "parameters": {
                    "Region": { "required": false, "type": "String", "builtIn": "AWS::Region" },
                }
            })
            service TestService {
                version: "2023-01-01",
                operations: [SomeOperation]
            }

            structure SomeOutput {
                someAttribute: Long,
                someVal: String
            }

            @http(uri: "/SomeOperation", method: "GET")
            @optionalAuth
            operation SomeOperation {
                output: SomeOutput
            }
        """.asSmithyModel()
    }

    @Test
    fun smokeTestSdkCodegen() {
        awsSdkIntegrationTest(model) { _, _ -> /* it should compile */ }
    }

    // TODO(PostGA): Remove warning banner conditionals.
    @Test
    fun warningBanners() {
        // Unstable version
        awsSdkIntegrationTest(
            model,
            params = awsIntegrationTestParams().copy(moduleVersion = "0.36.0"),
        ) { _, rustCrate ->
            rustCrate.integrationTest("banner") {
                rust(
                    """
                    ##[test]
                    fn banner() {
                        use std::process::Command;
                        use std::path::Path;

                        // Verify we're in the right directory
                        assert!(Path::new("Cargo.toml").try_exists().unwrap());
                        let output = Command::new("grep").arg("developer preview").arg("-i").arg("-R").arg(".").output().unwrap();
                        assert_eq!(0, output.status.code().unwrap(), "it should output the banner");
                    }
                    """,
                )
            }
        }

        // Stable version
        awsSdkIntegrationTest(
            model,
            params = awsIntegrationTestParams().copy(moduleVersion = "1.0.0"),
        ) { _, rustCrate ->
            rustCrate.integrationTest("no_banner") {
                rust(
                    """
                    ##[test]
                    fn banner() {
                        use std::process::Command;
                        use std::path::Path;

                        // Verify we're in the right directory
                        assert!(Path::new("Cargo.toml").try_exists().unwrap());
                        let output = Command::new("grep").arg("developer preview").arg("-i").arg("-R").arg(".").output().unwrap();
                        assert_eq!(1, output.status.code().unwrap(), "it should _not_ output the banner");
                    }
                    """,
                )
            }
        }
    }
}
