/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import CrateSet.AWS_SDK_RUNTIME
import CrateSet.SERVER_SMITHY_RUNTIME
import CrateSet.SMITHY_RUNTIME_COMMON
import CrateSet.STABLE_VERSION_PROP_NAME
import CrateSet.UNSTABLE_VERSION_PROP_NAME
import com.moandjiezana.toml.Toml
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import java.io.File
import java.util.logging.Logger

class CrateSetTest {
    private val logger: Logger = Logger.getLogger("CrateSetTest")

    /*
     * Checks whether `versionPropertyName` for a system under test, i.e. `Crate` in `CrateSet.kt`,
     * matches what `package.metadata.smithy-rs-release-tooling` says in the `Cargo.toml`
     * for the corresponding crate.
     */
    private fun sutStabilityMatchesManifestStability(versionPropertyName: String, stabilityInManifest: Boolean) {
        when (stabilityInManifest) {
            true -> assertEquals(STABLE_VERSION_PROP_NAME, versionPropertyName)
            false -> assertEquals(UNSTABLE_VERSION_PROP_NAME, versionPropertyName)
        }
    }

    /*
     * Checks whether each element in `crateSet` specifies the correct `versionPropertyName` according to
     * what `package.metadata.smithy-rs-release-tooling` says in the `Cargo.toml` for the corresponding crate,
     * located at `relativePathToRustRuntime`.
     *
     * If `package.metadata.smithy-rs-release-tooling` does not exist in a `Cargo.toml`, the implementation
     * will treat that crate as unstable.
     */
    private fun crateSetStabilitiesMatchManifestStabilities(crateSet: List<Crate>, relativePathToRustRuntime: String) {
        crateSet.forEach {
            val path = "$relativePathToRustRuntime/${it.name}/Cargo.toml"
            val contents = File(path).readText()
            val manifest = try {
                Toml().read(contents)
            } catch (e: java.lang.IllegalStateException) {
                // Currently, `aws-sigv4` cannot be read as a `TOML` because of the following error:
                // Invalid table definition on line 54: [target.'cfg(not(any(target_arch = "powerpc", target_arch = "powerpc64")))'.dev-dependencies]]
                logger.info("failed to read ${it.name} as TOML: $e")
                Toml()
            }
            manifest.getTable("package.metadata.smithy-rs-release-tooling")?.entrySet()?.map { entry ->
                sutStabilityMatchesManifestStability(it.versionPropertyName, entry.value as Boolean)
            } ?: sutStabilityMatchesManifestStability(it.versionPropertyName, false)
        }
    }

    @Test
    fun `aws runtime stabilities should match those in manifest files`() {
        crateSetStabilitiesMatchManifestStabilities(AWS_SDK_RUNTIME, "../aws/rust-runtime")
    }

    @Test
    fun `common smithy runtime stabilities should match those in manifest files`() {
        crateSetStabilitiesMatchManifestStabilities(SMITHY_RUNTIME_COMMON, "../rust-runtime")
    }

    @Test
    fun `server smithy runtime stabilities should match those in manifest files`() {
        crateSetStabilitiesMatchManifestStabilities(SERVER_SMITHY_RUNTIME, "../rust-runtime")
    }
}
