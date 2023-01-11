/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.transformers

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.core.util.findStreamingMember
import software.amazon.smithy.rust.codegen.core.util.orNull
import java.util.logging.Logger

// https://github.com/awslabs/smithy-rs/pull/1944#discussion_r1047800762
// TODO
object SkipSerde {
    private val logger = Logger.getLogger(javaClass.name)

    fun transform(model: Model, settings: ClientRustSettings): Model {
        // If Event Stream is allowed in build config, then don't remove the operations
        
        
        return ModelTransformer.create().filterShapes(model) { parentShape ->
            // TODO!
        }
    }
}
 