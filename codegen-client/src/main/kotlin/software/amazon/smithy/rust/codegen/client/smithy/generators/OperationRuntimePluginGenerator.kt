/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

/**
 * Generates operation-level runtime plugins
 */
class OperationRuntimePluginGenerator(
    codegenContext: ClientCodegenContext,
) {
    private val codegenScope = codegenContext.runtimeConfig.let { rc ->
        arrayOf(
            "BoxError" to RuntimeType.smithyRuntimeApi(rc).resolve("client::runtime_plugin::BoxError"),
            "ConfigBag" to RuntimeType.smithyRuntimeApi(rc).resolve("config_bag::ConfigBag"),
            "ConfigBagAccessors" to RuntimeType.smithyRuntimeApi(rc).resolve("client::orchestrator::ConfigBagAccessors"),
            "RuntimePlugin" to RuntimeType.smithyRuntimeApi(rc).resolve("client::runtime_plugin::RuntimePlugin"),
        )
    }

    fun render(writer: RustWriter, operationStructName: String) {
        writer.rustTemplate(
            """
            impl #{RuntimePlugin} for $operationStructName {
                fn configure(&self, cfg: &mut #{ConfigBag}) -> Result<(), #{BoxError}> {
                    use #{ConfigBagAccessors} as _;
                    cfg.set_request_serializer(${operationStructName}RequestSerializer);
                    cfg.set_response_deserializer(${operationStructName}ResponseDeserializer);
                    Ok(())
                }
            }
            """,
            *codegenScope,
        )
    }
}
