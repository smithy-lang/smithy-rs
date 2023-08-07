/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.glacier

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.operationBuildError
import software.amazon.smithy.rustsdk.InlineAwsDependency

// TODO(enableNewSmithyRuntimeCleanup): Delete this file when cleaning up middleware.

val TreeHashDependencies = listOf(
    CargoDependency.Ring,
    CargoDependency.TokioStream,
    CargoDependency.BytesUtils,
    CargoDependency.Bytes,
    CargoDependency.Tokio,
    CargoDependency.Hex,
    CargoDependency.TempFile,
)

private val UploadArchive: ShapeId = ShapeId.from("com.amazonaws.glacier#UploadArchive")
private val UploadMultipartPart: ShapeId = ShapeId.from("com.amazonaws.glacier#UploadMultipartPart")
private val Applies = setOf(UploadArchive, UploadMultipartPart)

class TreeHashHeader(private val runtimeConfig: RuntimeConfig) : OperationCustomization() {
    private val glacierChecksums = RuntimeType.forInlineDependency(
        InlineAwsDependency.forRustFile(
            "glacier_checksums",
            additionalDependency = TreeHashDependencies.toTypedArray(),
        ),
    )

    override fun section(section: OperationSection): Writable {
        return when (section) {
            is OperationSection.MutateRequest -> writable {
                rustTemplate(
                    """
                    #{glacier_checksums}::add_checksum_treehash(
                        &mut ${section.request}
                    ).await.map_err(#{BuildError}::other)?;
                    """,
                    "glacier_checksums" to glacierChecksums, "BuildError" to runtimeConfig.operationBuildError(),
                )
            }

            else -> emptySection
        }
    }

    companion object {
        fun forOperation(operation: OperationShape, runtimeConfig: RuntimeConfig): TreeHashHeader? {
            return if (Applies.contains(operation.id)) {
                TreeHashHeader(runtimeConfig)
            } else {
                null
            }
        }
    }
}
