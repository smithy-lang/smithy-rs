/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.error

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup

internal class ServiceErrorGeneratorTest {
    private val model = """
        namespace com.example

        use aws.protocols#restJson1

        @restJson1
        service HelloService {
            operations: [SayHello],
            version: "1"
        }

        @http(uri: "/", method: "POST")
        operation SayHello {
            input: EmptyStruct,
            output: EmptyStruct,
            errors: [SorryBusy, CanYouRepeatThat, MeDeprecated]
        }

        structure EmptyStruct { }

        @error("server")
        structure SorryBusy { }

        @error("client")
        structure CanYouRepeatThat { }

        @error("client")
        @deprecated
        structure MeDeprecated { }
    """.asSmithyModel()

    @Test
    fun `top level errors are send + sync`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.integrationTest("validate_errors") {
                rust(
                    """
                    fn check_send_sync<T: Send + Sync>() {}

                    ##[test]
                    fn service_errors_are_send_sync() {
                        check_send_sync::<${codegenContext.moduleUseName()}::Error>()
                    }
                    """,
                )
            }
        }
    }

    @Test
    fun `generates combined error enums`() {
        clientIntegrationTest(model) { _, rustCrate ->
            rustCrate.moduleFor(model.lookup<StructureShape>("com.example#CanYouRepeatThat")) {
                unitTest(
                    name = "generates_combined_error_enums",
                    test = """
                        use std::error::Error as StdError;
                        use crate::Error;
                        use crate::operation::say_hello::SayHelloError;

                        // Unhandled variants properly delegate source.
                        let error = Error::from(SayHelloError::unhandled("some other error"));
                        let source = error.source().expect("source should not be None");
                        assert_eq!(format!("{}", source), "some other error");
                    """,
                )
            }
        }
    }
}
