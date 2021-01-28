/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.util.lookup
import software.amazon.smithy.rust.testutil.asSmithyModel
import software.amazon.smithy.rust.testutil.compileAndTest
import software.amazon.smithy.rust.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.testutil.testSymbolProvider

internal class CombinedErrorGeneratorTest {
    @Test
    fun `generate combined error enums`() {
        val model = """
        namespace error
        operation Greeting {
            errors: [InvalidGreeting, ComplexError, FooError]
        }

        @error("client")
        @retryable
        structure InvalidGreeting {
            message: String,
        }

        @error("server")
        structure FooError {}

        @error("server")
        structure ComplexError {
            abc: String,
            other: Integer
        }
        """.asSmithyModel()
        val symbolProvider = testSymbolProvider(model)
        val writer = RustWriter.forModule("error")
        listOf("FooError", "ComplexError", "InvalidGreeting").forEach {
            model.lookup<StructureShape>("error#$it").renderWithModelBuilder(model, symbolProvider, writer)
        }
        val generator = CombinedErrorGenerator(model, testSymbolProvider(model), model.lookup("error#Greeting"))
        generator.render(writer)
        println(writer)
        writer.compileAndTest(
            """
            let error = GreetingError::InvalidGreeting(InvalidGreeting::builder().message("an error").build());
            assert_eq!(format!("{}", error), "InvalidGreeting: an error");
            assert_eq!(error.message(), Some("an error"));
            assert_eq!(error.code(), Some("InvalidGreeting"));
            use smithy_types::retry::ProvideErrorKind;
            assert_eq!(error.error_kind(), Some(smithy_types::retry::ErrorKind::ClientError));


            // unhandled variants properly delegate message
            let error = GreetingError::Unhandled(Box::new(smithy_types::Error {
               code: None,
               message: Some("hello".to_string()),
               request_id: None
            }));
            assert_eq!(error.message(), Some("hello"));

            let error = GreetingError::unhandled("some other error");
            assert_eq!(error.message(), None);
            assert_eq!(error.code(), None);
        """
        )
    }
}
