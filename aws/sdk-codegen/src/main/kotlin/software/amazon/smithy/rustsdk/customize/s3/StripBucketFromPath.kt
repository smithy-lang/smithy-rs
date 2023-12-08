/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.s3

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.pattern.UriPattern
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.core.util.letIf

class StripBucketFromHttpPath {
    private val transformer = ModelTransformer.create()

    fun transform(model: Model): Model {
        // Remove `/{Bucket}` from the path (http trait)
        // The endpoints 2.0 rules handle either placing the bucket into the virtual host or adding it to the path
        return transformer.mapTraits(model) { shape, trait ->
            when (trait) {
                is HttpTrait -> {
                    val appliedToOperation =
                        shape
                            .asOperationShape()
                            .map { operation ->
                                model.expectShape(operation.inputShape, StructureShape::class.java)
                                    .getMember("Bucket").isPresent
                            }.orElse(false)
                    trait.letIf(appliedToOperation) {
                        it.toBuilder().uri(UriPattern.parse(transformUri(trait.uri.toString()))).build()
                    }
                }

                else -> trait
            }
        }
    }

    private fun transformUri(uri: String): String {
        if (!uri.startsWith("/{Bucket}")) {
            throw IllegalStateException("tried to transform `$uri` that was not a standard bucket URI")
        }
        val withoutBucket = uri.replace("/{Bucket}", "")
        return if (!withoutBucket.startsWith("/")) {
            "/$withoutBucket"
        } else {
            withoutBucket
        }
    }
}
