/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.apigateway

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.rustlang.Writable
import software.amazon.smithy.rust.codegen.client.rustlang.rust
import software.amazon.smithy.rust.codegen.client.rustlang.writable
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.client.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.letIf

class ApiGatewayDecorator : RustCodegenDecorator<ClientCodegenContext> {
    override val name: String = "ApiGateway"
    override val order: Byte = 0

    private fun applies(coreCodegenContext: CoreCodegenContext) =
        coreCodegenContext.serviceShape.id == ShapeId.from("com.amazonaws.apigateway#BackplaneControlService")

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return baseCustomizations.letIf(applies(codegenContext)) {
            it + ApiGatewayAddAcceptHeader()
        }
    }

    override fun supportsCodegenContext(clazz: Class<out CoreCodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)
}

class ApiGatewayAddAcceptHeader : OperationCustomization() {
    override fun section(section: OperationSection): Writable = when (section) {
        is OperationSection.FinalizeOperation -> emptySection
        is OperationSection.OperationImplBlock -> emptySection
        is OperationSection.MutateRequest -> writable {
            rust(
                """${section.request}
                .http_mut()
                .headers_mut()
                .insert("Accept", #T::HeaderValue::from_static("application/json"));""",
                RuntimeType.http,
            )
        }
        else -> emptySection
    }
}
