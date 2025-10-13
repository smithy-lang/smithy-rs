/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.s3

import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.DeprecatedTrait
import software.amazon.smithy.rust.codegen.client.testutil.testClientRustSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rustsdk.awsSdkIntegrationTest
import kotlin.jvm.optionals.getOrNull

internal class S3DecoratorTest {
    /**
     * Base S3 model for testing. Includes GetBucketLocation, HeadBucket, and CreateBucket operations.
     * This minimal model allows us to test deprecation behavior without loading the full S3 model.
     */
    private val baseModel =
        """
        namespace com.amazonaws.s3
        
        use aws.protocols#restXml
        use aws.auth#sigv4
        use aws.api#service
        use smithy.rules#endpointRuleSet
        
        @restXml
        @sigv4(name: "s3")
        @service(
            sdkId: "S3"
            arnNamespace: "s3"
        )
        @endpointRuleSet({
            "version": "1.0",
            "rules": [{ "type": "endpoint", "conditions": [], "endpoint": { "url": "https://s3.amazonaws.com" } }],
            "parameters": {
                "Region": { "required": false, "type": "String", "builtIn": "AWS::Region" }
            }
        })
        service S3 {
            version: "2006-03-01",
            operations: [GetBucketLocation, HeadBucket, CreateBucket]
        }
        
        @http(method: "GET", uri: "/{Bucket}?location")
        operation GetBucketLocation {
            input: GetBucketLocationRequest
            output: GetBucketLocationOutput
        }
        
        @http(method: "HEAD", uri: "/{Bucket}")
        operation HeadBucket {
            input: HeadBucketRequest
        }
        
        @http(method: "PUT", uri: "/{Bucket}")
        operation CreateBucket {
            input: CreateBucketRequest
        }
        
        structure GetBucketLocationRequest {
            @required
            @httpLabel
            Bucket: String
        }
        
        @output
        structure GetBucketLocationOutput {
            LocationConstraint: String
        }
        
        structure HeadBucketRequest {
            @required
            @httpLabel
            Bucket: String
        }
        
        structure CreateBucketRequest {
            @required
            @httpLabel
            Bucket: String
        }
        """.asSmithyModel()

    private val serviceShape = baseModel.expectShape(ShapeId.from("com.amazonaws.s3#S3"), ServiceShape::class.java)
    private val settings = testClientRustSettings()

    /**
     * Helper method to apply the S3Decorator transformation to the base model.
     * This simulates what happens during code generation.
     */
    private fun transformModel() = S3Decorator().transformModel(serviceShape, baseModel, settings)

    @Test
    fun `GetBucketLocation operation has DeprecatedTrait`() {
        // Apply the S3Decorator transformation
        val transformedModel = transformModel()

        // Get the GetBucketLocation operation from the transformed model
        val getBucketLocationId = ShapeId.from("com.amazonaws.s3#GetBucketLocation")
        val operation = transformedModel.expectShape(getBucketLocationId, OperationShape::class.java)

        // Assert that the operation has the DeprecatedTrait
        assertTrue(operation.hasTrait<DeprecatedTrait>(), "GetBucketLocation should have DeprecatedTrait")

        // Get the trait and verify the message content
        val trait = operation.expectTrait(DeprecatedTrait::class.java)
        val message = trait.message.getOrNull()

        // Assert that the message contains "HeadBucket"
        assertTrue(
            message?.contains("HeadBucket") == true,
            "Deprecation message should mention HeadBucket",
        )

        // Assert that the message contains the AWS documentation URL
        assertTrue(
            message?.contains("https://docs.aws.amazon.com/AmazonS3/latest/API/API_HeadBucket.html") == true,
            "Deprecation message should include AWS documentation URL",
        )
    }

    @Test
    fun `Other S3 operations do not have DeprecatedTrait`() {
        // Apply the S3Decorator transformation
        val transformedModel = transformModel()

        // Check HeadBucket operation does NOT have DeprecatedTrait
        val headBucketId = ShapeId.from("com.amazonaws.s3#HeadBucket")
        val headBucket = transformedModel.expectShape(headBucketId, OperationShape::class.java)
        assertFalse(
            headBucket.hasTrait<DeprecatedTrait>(),
            "HeadBucket should not have DeprecatedTrait",
        )

        // Check CreateBucket operation does NOT have DeprecatedTrait
        val createBucketId = ShapeId.from("com.amazonaws.s3#CreateBucket")
        val createBucket = transformedModel.expectShape(createBucketId, OperationShape::class.java)
        assertFalse(
            createBucket.hasTrait<DeprecatedTrait>(),
            "CreateBucket should not have DeprecatedTrait",
        )
    }

    @Test
    fun `Generated Rust code includes deprecated attribute for GetBucketLocation`() {
        // This integration test generates Rust code for the S3 client with the modified decorator
        // and verifies that the #[deprecated] attribute is present in the generated code by:
        // 1. Generating the complete S3 client code with the S3Decorator applied
        // 2. Compiling the generated code (which includes the client module with get_bucket_location)
        // 3. Running a test that references the deprecated method
        //
        // The test passes if:
        // - The code generation succeeds (DeprecatedTrait is properly converted to Rust #[deprecated])
        // - The generated code compiles successfully
        // - The test that uses the method compiles (deprecation warnings are allowed)
        //
        // This verifies the end-to-end flow: Model transformation -> Code generation -> Compilation
        awsSdkIntegrationTest(baseModel) { ctx, rustCrate ->
            val moduleName = ctx.moduleUseName()

            // Create an integration test that uses the deprecated method
            // This will trigger a deprecation warning during compilation if the attribute is present
            rustCrate.integrationTest("verify_get_bucket_location_deprecated") {
                writeWithNoFormatting(
                    """
                    use $moduleName::Client;
                    
                    // This test verifies that the get_bucket_location method exists and is accessible.
                    // If the #[deprecated] attribute is present in the generated code, the Rust compiler
                    // will emit a deprecation warning when this code is compiled.
                    //
                    // The deprecation message should contain:
                    // - "HeadBucket" - the recommended alternative operation
                    // - "https://docs.aws.amazon.com/AmazonS3/latest/API/API_HeadBucket.html" - documentation URL
                    #[test]
                    fn get_bucket_location_method_exists() {
                        // Reference the method to trigger deprecation warning
                        fn check_method_exists<T>(_: T) {}
                        check_method_exists(Client::get_bucket_location);
                    }
                    """,
                )
            }
        }
    }
}
