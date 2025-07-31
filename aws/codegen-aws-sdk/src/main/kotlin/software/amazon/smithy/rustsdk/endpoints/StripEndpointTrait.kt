/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.endpoints

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.traits.EndpointTrait
import software.amazon.smithy.model.transform.ModelTransformer

fun stripEndpointTrait(hostPrefix: String): (Model) -> Model {
    return { model: Model ->
        ModelTransformer.create()
            .removeTraitsIf(model) { _, trait ->
                trait is EndpointTrait &&
                    trait.hostPrefix.labels.any {
                        it.isLabel && it.content == hostPrefix
                    }
            }
    }
}
