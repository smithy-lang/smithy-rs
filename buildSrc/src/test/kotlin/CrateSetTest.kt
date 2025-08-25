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
    private fun sutStabilityMatchesManifestStability(
        versionPropertyName: String,
        stabilityInManifest: Boolean,
        crate: String,
    ) {
        when (stabilityInManifest) {
            true -> assertEquals(STABLE_VERSION_PROP_NAME, versionPropertyName, "Crate: $crate")
            false -> assertEquals(UNSTABLE_VERSION_PROP_NAME, versionPropertyName, "Crate: $crate")
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
    private fun crateSetStabilitiesMatchManifestStabilities(
        crateSet: List<Crate>,
        relativePathToRustRuntime: String,
    ) {
        crateSet.forEach {
            val path = "$relativePathToRustRuntime/${it.name}/Cargo.toml"
            val contents = File(path).readText()
            val isStable =
                try {
                    Toml().read(contents).getTable("package.metadata.smithy-rs-release-tooling")?.getBoolean("stable")
                        ?: false
                } catch (e: java.lang.IllegalStateException) {
                    // Several crates are stable, but their Cargo.toml does not properly parse due to cfg statements
                    // like `[target.'cfg(not(target_family = "wasm"))'.dependencies]`. Those packages are handled here
                    contents.contains("[package.metadata.smithy-rs-release-tooling]\nstable = true")
                }
            sutStabilityMatchesManifestStability(it.versionPropertyName, isStable, it.name)
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
