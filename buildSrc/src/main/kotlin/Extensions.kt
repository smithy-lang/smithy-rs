/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
import org.gradle.api.Project
import org.gradle.api.file.Directory
import org.gradle.api.provider.Provider

// JReleaser publishes artifacts from a local staging repository, rather than maven local.
// https://jreleaser.org/guide/latest/examples/maven/staging-artifacts.html#_gradle
fun Project.stagingDir(): Provider<Directory> {
    return rootProject.layout.buildDirectory.dir("m2")
}
