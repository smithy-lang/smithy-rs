/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.Version
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

class CargoTomlGeneratorTest {
    private val CargoMetadata: CargoDependency = CargoDependency("cargo_metadata", CratesIo("0.15.0"))

    @Test
    fun `adds codegen version to package metadata`() {
        val project = TestWorkspace.testProject()
        project.lib {
            addDependency(CargoMetadata)
            unitTest(
                "smithy_codegen_version_in_package_metadata",
                """
                let metadata = cargo_metadata::MetadataCommand::new()
                    .exec()
                    .expect("could not run `cargo metadata`");

                let pgk_metadata = &metadata.root_package().expect("missing root package").metadata;

                let codegen_version = pgk_metadata
                    .get("smithy")
                    .and_then(|s| s.get("codegen-version"))
                    .expect("missing `smithy.codegen-version` field")
                    .as_str()
                    .expect("`smithy.codegen-version` is not str");
                assert_eq!(codegen_version, "${Version.fullVersion()}");
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `check serde features`() {
        val project = TestWorkspace.testProject()
        /*
        ["target.'cfg(aws_sdk_unstable)'.dependencies".serde]
        version = "1.0"
        features = ["derive"]
        serde-serialize = ["aws-smithy-types/serde-serialize"]
        serde-deserialize = ["aws-smithy-types/serde-deserialize"]
        */
        project.lib {
            addDependency(CargoMetadata)
            unitTest(
                "smithy_codegen_serde_features",
                """
                let metadata = cargo_metadata::MetadataCommand::new()
                    .exec()
                    .expect("could not run `cargo metadata`");

                let features = &metadata.root_package().expect("missing root package").features;

                assert_eq!(features.get("aws-aws-smithy-types"), Some(vec!["serde-serialize", "serde-deserialize"]));
                """,
            )
        }
        project.compileAndTest()
    }
}
