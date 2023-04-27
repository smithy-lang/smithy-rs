/*
* Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
* SPDX-License-Identifier: Apache-2.0
*/

package software.amazon.smithy.rust.codegen.client.smithy.customize

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate

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
            return Feature(feature_name, false, listOf(crate_name + "/" + feature_name))
        }
        rustCrate.mergeFeature(_feature("serde-serialize", "aws-smithy-types"))
        rustCrate.mergeFeature(_feature("serde-deserialize", "aws-smithy-types"))
    }
}
