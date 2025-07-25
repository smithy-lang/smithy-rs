/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.glacier

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureSection
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rustsdk.AwsCargoDependency
import software.amazon.smithy.rustsdk.InlineAwsDependency

/**
 * Glacier has three required customizations:
 *
 * 1. Add the `x-amz-glacier-version` header to all requests
 * 2. For operations with an `account_id` field, autofill that field with `-` if it is not set by the customer.
 *    This is required because the account ID is sent across as part of the URL, and `-` has been pre-established
 *    "no account ID" in the Glacier service.
 * 3. The `UploadArchive` and `UploadMultipartPart` operations require tree hash headers to be
 *    calculated and added to the request.
 *
 * This decorator wires up these three customizations.
 */
class GlacierDecorator : ClientCodegenDecorator {
    override val name: String = "Glacier"
    override val order: Byte = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> = baseCustomizations + GlacierOperationInterceptorsCustomization(codegenContext)

    override fun structureCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<StructureCustomization>,
    ): List<StructureCustomization> = baseCustomizations + GlacierAccountIdCustomization(codegenContext)

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> = baseCustomizations + GlacierApiVersionCustomization(codegenContext)
}

/** Implements the `GlacierAccountId` trait for inputs that have an `account_id` field */
private class GlacierAccountIdCustomization(private val codegenContext: ClientCodegenContext) :
    StructureCustomization() {
    override fun section(section: StructureSection): Writable =
        writable {
            if (section is StructureSection.AdditionalTraitImpls && section.shape.inputWithAccountId()) {
                val inlineModule = inlineModule(codegenContext.runtimeConfig)
                rustTemplate(
                    """
                    impl #{GlacierAccountId} for ${section.structName} {
                        fn account_id_mut(&mut self) -> &mut Option<String> {
                            &mut self.account_id
                        }
                    }
                    """,
                    "GlacierAccountId" to inlineModule.resolve("GlacierAccountId"),
                )
            }
        }
}

/** Adds the `x-amz-glacier-version` header to all requests */
private class GlacierApiVersionCustomization(private val codegenContext: ClientCodegenContext) :
    ServiceRuntimePluginCustomization() {
    override fun section(section: ServiceRuntimePluginSection): Writable =
        writable {
            if (section is ServiceRuntimePluginSection.RegisterRuntimeComponents) {
                val apiVersion = codegenContext.serviceShape.version
                section.registerInterceptor(this) {
                    rustTemplate(
                        "#{Interceptor}::new(${apiVersion.dq()})",
                        "Interceptor" to inlineModule(codegenContext.runtimeConfig).resolve("GlacierApiVersionInterceptor"),
                    )
                }
            }
        }
}

/**
 * Adds two interceptors:
 * 1. `GlacierAccountIdAutofillInterceptor`: Uses the `GlacierAccountId` trait to correctly autofill the account ID field
 * 2. `GlacierSetPrecomputedSignableBodyInterceptor`: Reuses the tree hash computation during signing rather than allowing
 *    the `aws-sigv4` module to recalculate the payload hash.
 */
private class GlacierOperationInterceptorsCustomization(private val codegenContext: ClientCodegenContext) :
    OperationCustomization() {
    override fun section(section: OperationSection): Writable =
        writable {
            if (section is OperationSection.AdditionalInterceptors) {
                val inputShape = codegenContext.model.expectShape(section.operationShape.inputShape) as StructureShape
                val inlineModule = inlineModule(codegenContext.runtimeConfig)
                if (inputShape.inputWithAccountId()) {
                    section.registerInterceptor(codegenContext.runtimeConfig, this) {
                        rustTemplate(
                            "#{Interceptor}::<#{Input}>::new()",
                            "Interceptor" to inlineModule.resolve("GlacierAccountIdAutofillInterceptor"),
                            "Input" to codegenContext.symbolProvider.toSymbol(inputShape),
                        )
                    }
                }
                if (section.operationShape.requiresTreeHashHeader()) {
                    section.registerInterceptor(codegenContext.runtimeConfig, this) {
                        rustTemplate(
                            "#{Interceptor}::default()",
                            "Interceptor" to inlineModule.resolve("GlacierTreeHashHeaderInterceptor"),
                        )
                    }
                }
            }
        }
}

/** True when the operation requires tree hash headers */
private fun OperationShape.requiresTreeHashHeader(): Boolean =
    id == ShapeId.from("com.amazonaws.glacier#UploadArchive") ||
        id == ShapeId.from("com.amazonaws.glacier#UploadMultipartPart")

private fun StructureShape.inputWithAccountId(): Boolean =
    hasTrait<SyntheticInputTrait>() && members().any { it.memberName.lowercase() == "accountid" }

private fun inlineModule(runtimeConfig: RuntimeConfig) =
    RuntimeType.forInlineDependency(
        InlineAwsDependency.forRustFile(
            "glacier_interceptors",
            additionalDependency = glacierInterceptorDependencies(runtimeConfig).toTypedArray(),
        ),
    )

private fun glacierInterceptorDependencies(runtimeConfig: RuntimeConfig) =
    listOf(
        AwsCargoDependency.awsRuntime(runtimeConfig),
        AwsCargoDependency.awsSigv4(runtimeConfig),
        CargoDependency.Bytes,
        CargoDependency.Hex,
        CargoDependency.Ring,
        CargoDependency.smithyHttp(runtimeConfig),
        CargoDependency.smithyRuntimeApiClient(runtimeConfig),
        CargoDependency.Http1x,
    )
