/*
* Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
* SPDX-License-Identifier: Apache-2.0
*/

package software.amazon.smithy.rust.codegen.client.smithy.customize

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customizations.EndpointPrefixGenerator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.HttpChecksumRequiredGenerator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.HttpVersionListCustomization
import software.amazon.smithy.rust.codegen.client.smithy.customizations.IdempotencyTokenGenerator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.ResiliencyConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.customizations.ResiliencyReExportCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customizations.AllowLintsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customizations.CrateVersionCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customizations.pubUseSmithyTypes
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext

/**
 * This class,
 * - Adds serde as a dependency
 *
 */
class SerdeDecorator : ClientCodegenDecorator {
    override val name: String = "SerdeDecorator"
    override val order: Byte = -1

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        fun _feature(feature_name: String, crate_name: String): Feature {
            return Feature(feature_name, false, listOf(crate_name+"/"+feature_name))
        }
        rustCrate.mergeFeature(_feature("serde-serialize", "aws-sdk-types"))
        rustCrate.mergeFeature(_feature("serde-deserialize", "aws-sdk-types"))
    }
}
