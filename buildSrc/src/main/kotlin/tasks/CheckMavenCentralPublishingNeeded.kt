/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package tasks

import org.gradle.api.DefaultTask
import org.gradle.api.tasks.TaskAction
import java.net.HttpURLConnection
import java.net.URL

/**
 * Gradle task that checks if publishing to Maven Central is needed.
 * This task checks if the current version is a SNAPSHOT or if it already exists in Maven Central.
 * The result is written to a properties file that can be read by the GitHub workflow.
 */
open class CheckMavenCentralPublishingNeeded : DefaultTask() {
    init {
        description = "Check if publishing to Maven Central is needed"
    }

    @TaskAction
    fun check() {
        val codegenVersion = project.properties["codegenVersion"].toString()
        val outputDir = project.layout.buildDirectory.dir("maven-central").get().asFile
        outputDir.mkdirs()
        val outputFile = outputDir.resolve("publishing.properties")

        // Check if version contains SNAPSHOT
        if (codegenVersion.contains("-SNAPSHOT")) {
            logger.lifecycle("Version $codegenVersion is a SNAPSHOT, skipping Maven Central publishing")
            outputFile.writeText("mavenCentralPublishingNeeded=false")
            return
        }

        val artifactName = "codegen-core"
        // Check if version already exists in Maven Central
        val url = "https://repo1.maven.org/maven2/software/amazon/smithy/rust/$artifactName/$codegenVersion/$artifactName-$codegenVersion.pom"
        val connection = URL(url).openConnection() as HttpURLConnection
        connection.requestMethod = "HEAD"

        try {
            connection.connect()
            val responseCode = connection.responseCode

            if (responseCode == 200) {
                logger.lifecycle("Version $codegenVersion already exists in Maven Central, skipping publishing")
                outputFile.writeText("mavenCentralPublishingNeeded=false")
                return
            }
        } finally {
            connection.disconnect()
        }

        logger.lifecycle("Version $codegenVersion not found on Maven Central, release required")
        outputFile.writeText("mavenCentralPublishingNeeded=true")
    }
}
