/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package tasks

import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.Optional
import org.gradle.api.tasks.TaskAction
import java.io.ByteArrayOutputStream
import java.net.URL
import javax.xml.parsers.DocumentBuilderFactory

/**
 * Gradle task that verifies if the codegenVersion has been bumped when codegen files have changed.
 * This task checks if any files in the codegen projects have changed since the base reference,
 * and if they have, verifies that the codegenVersion in gradle.properties has been incremented.
 *
 * The task fetches the latest published version from Maven Central's maven-metadata.xml
 * to determine if the current version is newer.
 */
open class VerifyCodegenVersionBump : DefaultTask() {
    /**
     * The base git reference to compare against.
     * If not provided, the task will try to infer it from GitHub Actions environment variables,
     * or fall back to 'origin/main'.
     */
    @Input
    @Optional
    var baseRef: String? = null

    init {
        description = "Verify that codegenVersion has been bumped if codegen files have changed"
    }

    @TaskAction
    fun verify() {
        val publishedCodegenProjectPaths =
            listOf(
                "codegen-client", "codegen-core", "codegen-serde",
                "codegen-server", "fuzzgen", "aws/codegen-aws-sdk",
            )

        // Determine the base reference
        val effectiveBaseRef = determineBaseRef()
        logger.lifecycle("Using base reference: $effectiveBaseRef")

        // Check if any files in codegen projects have changed
        var hasChanges = false
        publishedCodegenProjectPaths.forEach { projectName ->
            val changesOutput = ByteArrayOutputStream()
            project.exec {
                commandLine("git", "diff", "--name-only", effectiveBaseRef, "HEAD", "--", projectName)
                standardOutput = changesOutput
            }

            if (changesOutput.toString().isNotEmpty()) {
                hasChanges = true
                logger.lifecycle("Changes detected in $projectName")
            }
        }

        if (!hasChanges) {
            logger.lifecycle("No changes detected in codegen projects, skipping version check")
            return
        }

        // Get current version from gradle.properties
        val codegenVersion = project.properties["codegenVersion"].toString()

        // Get the latest published version from Maven Central
        val latestPublishedVersion = getLatestPublishedVersion()

        logger.lifecycle("Current version: $codegenVersion")
        logger.lifecycle("Latest published version: $latestPublishedVersion")

        // Compare versions using semver comparison
        if (!isVersionBumped(latestPublishedVersion, codegenVersion)) {
            throw GradleException("Codegen files have changed but codegenVersion ($codegenVersion) has not been bumped from the latest published version ($latestPublishedVersion)")
        } else {
            logger.lifecycle("Version has been bumped from $latestPublishedVersion to $codegenVersion")
        }
    }

    /**
     * Determines the base reference to use for comparison.
     * Priority:
     * 1. Explicitly provided baseRef property
     * 2. GitHub Actions environment variables (GITHUB_BASE_REF)
     * 3. Fallback to 'origin/main'
     *
     * @return The base reference to use
     */
    private fun determineBaseRef(): String {
        // Use explicitly provided baseRef if available
        if (!baseRef.isNullOrBlank()) {
            return baseRef!!
        }

        // Try to get from GitHub Actions environment variables
        val githubBaseRef = System.getenv("GITHUB_BASE_REF")
        if (!githubBaseRef.isNullOrBlank()) {
            return "origin/$githubBaseRef"
        }

        // Try to get the default branch
        val defaultBranchOutput = ByteArrayOutputStream()
        project.exec {
            commandLine("git", "remote", "show", "origin")
            standardOutput = defaultBranchOutput
            isIgnoreExitValue = true
        }

        val defaultBranchLines = defaultBranchOutput.toString().lines()
        val headBranchLine = defaultBranchLines.find { it.contains("HEAD branch:") }
        if (headBranchLine != null) {
            val defaultBranch = headBranchLine.substringAfter("HEAD branch:").trim()
            return "origin/$defaultBranch"
        }

        // Fallback to origin/main
        return "origin/main"
    }

    /**
     * Fetches the latest published version from Maven Central's maven-metadata.xml.
     *
     * @return The latest published version string
     */
    private fun getLatestPublishedVersion(): String {
        try {
            val metadataUrl =
                URL("https://repo1.maven.org/maven2/software/amazon/smithy/rust/codegen-core/maven-metadata.xml")
            val connection = metadataUrl.openConnection()
            connection.connectTimeout = 5000
            connection.readTimeout = 5000

            val inputStream = connection.getInputStream()
            val dbFactory = DocumentBuilderFactory.newInstance()
            val dBuilder = dbFactory.newDocumentBuilder()
            val doc = dBuilder.parse(inputStream)
            doc.documentElement.normalize()

            val latestElement = doc.getElementsByTagName("latest").item(0)
            if (latestElement != null) {
                return latestElement.textContent
            }

            // If no "latest" tag, get the last "version" tag
            val versionElements = doc.getElementsByTagName("version")
            if (versionElements.length > 0) {
                return versionElements.item(versionElements.length - 1).textContent
            }

            throw GradleException("Could not find version information in maven-metadata.xml")
        } catch (e: Exception) {
            logger.warn("Failed to fetch latest version from Maven Central: ${e.message}")
            logger.warn("Falling back to version 0.0.0 for comparison")
            return "0.0.0"
        }
    }

    /**
     * Helper function to compare versions and determine if the new version is higher than the old version.
     * This is a simple implementation that can be enhanced with a proper semver comparison library if needed.
     *
     * @param oldVersion The old version string
     * @param newVersion The new version string
     * @return True if the new version is higher than the old version, false otherwise
     */
    private fun isVersionBumped(
        oldVersion: String,
        newVersion: String,
    ): Boolean {
        // Simple implementation - can be enhanced with proper semver comparison library
        val oldParts = oldVersion.split(".")
        val newParts = newVersion.split(".")

        for (i in 0 until minOf(oldParts.size, newParts.size)) {
            val oldPart = oldParts[i].toIntOrNull() ?: 0
            val newPart = newParts[i].toIntOrNull() ?: 0

            if (newPart > oldPart) return true
            if (newPart < oldPart) return false
        }

        return newParts.size > oldParts.size
    }
}
