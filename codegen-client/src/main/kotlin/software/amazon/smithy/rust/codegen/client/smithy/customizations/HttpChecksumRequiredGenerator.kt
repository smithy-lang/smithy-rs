/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.HttpChecksumRequiredTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.toType
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.core.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape

class HttpChecksumRequiredGenerator(
    private val codegenContext: CodegenContext,
    private val operationShape: OperationShape,
) : OperationCustomization() {
    override fun section(section: OperationSection): Writable {
        if (!operationShape.hasTrait<HttpChecksumRequiredTrait>()) {
            return emptySection
        }
        if (operationShape.inputShape(codegenContext.model).hasStreamingMember(codegenContext.model)) {
            throw CodegenException("HttpChecksum required cannot be applied to a streaming shape")
        }
        return when (section) {
            is OperationSection.AdditionalRuntimePlugins -> writable {
                section.addOperationRuntimePlugin(this) {
                    rustTemplate(
                        "#{HttpChecksumRequiredRuntimePlugin}::new()",
                        "HttpChecksumRequiredRuntimePlugin" to
                            InlineDependency.forRustFile(
                                RustModule.pubCrate("client_http_checksum_required", parent = ClientRustModule.root),
                                "/inlineable/src/client_http_checksum_required.rs",
                                CargoDependency.smithyRuntimeApi(codegenContext.runtimeConfig),
                                CargoDependency.smithyTypes(codegenContext.runtimeConfig),
                                CargoDependency.Http,
                                CargoDependency.Md5,
                            ).toType().resolve("HttpChecksumRequiredRuntimePlugin"),
                    )
                }
            }
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
                    "md5" to RuntimeType.Md5,
                    "http" to RuntimeType.Http,
                    "base64_encode" to RuntimeType.base64Encode(codegenContext.runtimeConfig),
                    "BuildError" to codegenContext.runtimeConfig.operationBuildError(),
                )
            }
            else -> emptySection
        }
    }
}
