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
    val modelHash: String? = null,
)

/** Kotlin representation of aws-sdk-rust's `versions.toml` file */
data class VersionsManifest(
    val smithyRsRevision: String,
    val crates: Map<String, CrateVersion>,
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
                crates =
                    toml.getTable("crates").entrySet().map { entry ->
                        val crate = (entry.value as Toml)
                        entry.key to
                            CrateVersion(
                                category = crate.getString("category"),
                                version = crate.getString("version"),
                                sourceHash = crate.getString("source_hash"),
                                modelHash = crate.getString("model_hash"),
                            )
                    }.toMap(),
            )
        }
    }
}
