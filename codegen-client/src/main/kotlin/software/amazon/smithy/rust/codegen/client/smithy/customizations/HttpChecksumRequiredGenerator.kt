/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.HttpChecksumRequiredTrait
import software.amazon.smithy.rust.codegen.client.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.client.rustlang.Writable
import software.amazon.smithy.rust.codegen.client.rustlang.asType
import software.amazon.smithy.rust.codegen.client.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.client.rustlang.writable
import software.amazon.smithy.rust.codegen.client.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.client.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.client.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.client.util.hasTrait
import software.amazon.smithy.rust.codegen.client.util.inputShape

class HttpChecksumRequiredGenerator(
    private val coreCodegenContext: CoreCodegenContext,
    private val operationShape: OperationShape,
) : OperationCustomization() {
    override fun section(section: OperationSection): Writable {
        if (!operationShape.hasTrait<HttpChecksumRequiredTrait>()) {
            return emptySection
        }
        if (operationShape.inputShape(coreCodegenContext.model).hasStreamingMember(coreCodegenContext.model)) {
            throw CodegenException("HttpChecksum required cannot be applied to a streaming shape")
        }
        return when (section) {
            is OperationSection.MutateRequest -> writable {
                rustTemplate(
                    """
                    ${section.request} = ${section.request}.augment(|mut req, _| {
                        let data = req
                            .body()
                            .bytes()
                            .expect("checksum can only be computed for non-streaming operations");
                        let checksum = <#{md5}::Md5 as #{md5}::Digest>::digest(data);
                        req.headers_mut().insert(
                            #{http}::header::HeaderName::from_static("content-md5"),
                            #{base64_encode}(&checksum[..]).parse().expect("checksum is valid header value")
                        );
                        Result::<_, #{BuildError}>::Ok(req)
                    })?;
                    """,
                    "md5" to CargoDependency.Md5.asType(),
                    "http" to CargoDependency.Http.asType(),
                    "base64_encode" to RuntimeType.Base64Encode(coreCodegenContext.runtimeConfig),
                    "BuildError" to coreCodegenContext.runtimeConfig.operationBuildError(),
                )
            }
            else -> emptySection
        }
    }
}
