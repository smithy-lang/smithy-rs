/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.rust.codegen.core.smithy.generators.ManifestCustomizations
import software.amazon.smithy.rust.codegen.server.ServerVersion

internal fun serverCodegenVersionMetadata(): ManifestCustomizations {
    val version = ServerVersion.fromDefaultResource()
    return mapOf(
        "package" to
            mapOf(
                "metadata" to
                    mapOf(
                        "smithy" to
                            mapOf(
                                "codegen-version" to version.codegenVersion,
                                "codegen-version-commit" to version.gitHash,
                            ),
                    ),
            ),
    )
}
