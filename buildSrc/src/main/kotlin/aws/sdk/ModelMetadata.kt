/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package aws.sdk

import com.moandjiezana.toml.Toml
import java.io.File

enum class ChangeType {
    UNCHANGED,
    FEATURE,
    DOCUMENTATION,
}

/** Model metadata toml file */
data class ModelMetadata(
    private val crates: Map<String, ChangeType>,
) {
    companion object {
        fun fromFile(path: String): ModelMetadata {
            val contents = File(path).readText()
            return fromString(contents)
        }

        fun fromString(value: String): ModelMetadata {
            val toml = Toml().read(value)
            return ModelMetadata(
                crates = toml.getTable("crates")?.entrySet()?.map { entry ->
                    entry.key to when (val kind = (entry.value as Toml).getString("kind")) {
                        "Feature" -> ChangeType.FEATURE
                        "Documentation" -> ChangeType.DOCUMENTATION
                        else -> throw IllegalArgumentException("Unrecognized change type: $kind")
                    }
                }?.toMap() ?: emptyMap(),
            )
        }
    }

    fun hasCrates(): Boolean = crates.isNotEmpty()
    fun changeType(moduleName: String): ChangeType = crates[moduleName] ?: ChangeType.UNCHANGED
}
