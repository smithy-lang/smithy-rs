/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package aws.sdk

import com.moandjiezana.toml.Toml
import java.io.File

data class CrateVersion(
    val category: String,
    val version: String,
    val sourceHash: String? = null,
    val modelHash: String? = null
)

/** Kotlin representation of aws-sdk-rust's `versions.toml` file */
data class VersionsManifest(
    val smithyRsRevision: String,
    val awsDocSdkExamplesRevision: String,
    val crates: Map<String, CrateVersion>
) {
    companion object {
        fun fromFile(path: String): VersionsManifest {
            val contents = File(path).readText()
            return fromString(contents)
        }

        fun fromString(value: String): VersionsManifest {
            val toml = Toml().read(value)
            return VersionsManifest(
                smithyRsRevision = toml.getString("smithy_rs_revision"),
                awsDocSdkExamplesRevision = toml.getString("aws_doc_sdk_examples_revision"),
                crates = toml.getTable("crates").entrySet().map { entry ->
                    val value = (entry.value as Toml)
                    entry.key to CrateVersion(
                        category = value.getString("category"),
                        version = value.getString("version"),
                        sourceHash = value.getString("source_hash"),
                        modelHash = value.getString("model_hash")
                    )
                }.toMap()
            )
        }
    }
}
