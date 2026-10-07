/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package tasks

import PublishedMavenArtifacts
import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.TaskAction
import java.io.File
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.TimeUnit

/**
 * Decides whether a release needs to publish to Maven Central, and refuses to let it publish a
 * coordinate that already exists.
 *
 * Maven Central coordinates are immutable, so `./gradlew publish` against an existing
 * `codegenVersion` fails — and it fails late, in the Sonatype upload, after the release pipeline
 * has already pushed a git tag and published crates. This task answers the question up front, from
 * two independent signals:
 *
 *  1. **git** — have any of the source paths that feed a published artifact changed since the
 *     release that introduced the current `codegenVersion`? See [PublishedMavenArtifacts].
 *  2. **repo1.maven.org** — are the artifacts for this `codegenVersion` already there?
 *
 * The two combine into four outcomes:
 *
 * | artifacts changed | already published | outcome                                    |
 * | ----------------- | ----------------- | ------------------------------------------ |
 * | no                | yes               | skip — nothing to publish (exit 0)         |
 * | no                | no                | publish — first release of this version    |
 * | yes               | yes               | **fail** — version must be bumped          |
 * | yes               | no                | publish — the ordinary release             |
 *
 * A needed bump is written to `build/maven-central/bump-needed.md`, which CI turns into a pull
 * request comment and the release pipeline turns into a failure. Nothing else is written: the
 * skip-or-publish decision belongs to the Maven publisher, which asks repo1 at the moment it would
 * upload, so it cannot go stale or disagree with what is actually being published.
 *
 * Only the "changed but already published" case is unfixable and therefore fatal: the artifacts
 * that landed cannot be replaced, so the release must move to a new version. Pass
 * `-PfailOnAlreadyPublished` to also fail the unchanged-but-published case, which turns this into
 * a strict gate that asserts every release publishes something new.
 *
 * Diagnostic overrides, for reproducing a past release locally:
 *  - `-PcodegenVersionOverride=X` — check version `X` instead of the one in `gradle.properties`
 *  - `-PheadRevOverride=<rev>` — treat `<rev>` as HEAD when comparing against the previous release
 */
open class CheckMavenCentralPublishingNeeded : DefaultTask() {
    init {
        description = "Check whether a release needs to publish to Maven Central"
        group = "verification"
        // The answer depends on the state of repo1.maven.org and of git, not just on our inputs.
        outputs.upToDateWhen { false }
    }

    @get:Input
    var connectTimeoutMillis: Int = 10_000

    @get:Input
    var readTimeoutMillis: Int = 10_000

    /** Pattern matching the release tags this repository cuts, e.g. `release-2026-09-30`. */
    @get:Input
    var releaseTagPattern: String = "release-*"

    @TaskAction
    fun check() {
        val codegenVersion =
            project.findProperty("codegenVersionOverride")?.toString()
                ?: project.properties["codegenVersion"].toString()
        val headRev = project.findProperty("headRevOverride")?.toString() ?: WORKING_TREE
        val strict = project.hasProperty("failOnAlreadyPublished")

        logger.lifecycle("Checking whether codegenVersion $codegenVersion needs publishing to Maven Central")

        if (codegenVersion.endsWith("-SNAPSHOT")) {
            logger.lifecycle("  codegenVersion $codegenVersion is a SNAPSHOT; Maven Central publishing is not applicable")
            return
        }

        val baseTag = previousReleaseAtSameVersion(headRev, codegenVersion)
        val changes = if (baseTag == null) null else changedArtifactSources(baseTag.name, headRev)
        val publishState = probePublishState(codegenVersion)

        logger.lifecycle(diagnosticReport(codegenVersion, headRev, baseTag, changes, publishState))

        // repo1 unreachable or answering oddly. Decide from git, which needs no network. A release
        // tag carrying this codegenVersion means a release already went out at it, so if anything
        // feeding a published artifact has changed since that tag, the version needs bumping no
        // matter what repo1 says. Anything else defers to the publisher, which runs its own check
        // against repo1 at release time. This must not fail: the task gates Matrix Success, so
        // failing here would block every pull request whenever Maven Central is down.
        if (publishState.isInconclusive) {
            val changedPaths = changes?.changedPaths.orEmpty()
            if (baseTag != null && changedPaths.isNotEmpty()) {
                reportBumpNeeded(needsBumpMessage(codegenVersion, baseTag, changes!!))
                return
            }
            logger.warn(
                "==> PUBLISH (unverified): repo1 gave no conclusive answer for " +
                    "${publishState.inconclusive.joinToString(", ")}, so this was decided from git " +
                    "alone. No source feeding a published artifact changed since " +
                    "${baseTag?.name ?: "any release at this version"}, so a bump is not required. " +
                    "The publisher checks Maven Central again before uploading.",
            )
            return
        }

        // A partially published version can never be completed: the artifacts that landed are
        // immutable, so the missing ones can never join them under the same coordinates.
        if (publishState.isPartial) {
            throw GradleException(partialPublicationMessage(codegenVersion, publishState))
        }

        if (!publishState.isFullyPublished) {
            logger.lifecycle(
                "==> PUBLISH: codegenVersion $codegenVersion is not on Maven Central, so the release " +
                    "should publish it.",
            )
            return
        }

        // Fully published from here on.
        val changedPaths = changes?.changedPaths.orEmpty()
        if (changedPaths.isNotEmpty()) {
            reportBumpNeeded(needsBumpMessage(codegenVersion, baseTag!!, changes!!))
            return
        }

        val since = baseTag?.let { "since ${it.name}" } ?: "since it was published"
        val nothingToDo =
            "codegenVersion $codegenVersion is already published to Maven Central and no source " +
                "that feeds a published artifact has changed $since, so there is nothing to " +
                "publish.\n\nThis is the expected state for a release that only changes runtime " +
                "crates, tooling, CI, or documentation. Those ship to crates.io, not to Maven Central."

        if (strict) {
            throw GradleException(
                "$nothingToDo\n\n" +
                    "Failing because -PfailOnAlreadyPublished was set. Drop that flag to let the " +
                    "release skip Maven Central instead.",
            )
        }

        logger.lifecycle("==> SKIP: $nothingToDo")
    }

    // ---------------------------------------------------------------------------------------------
    // git
    // ---------------------------------------------------------------------------------------------

    /** A release tag and the commit it points at. */
    data class ReleaseTag(val name: String, val commit: String)

    /**
     * Finds the release tag from which [codegenVersion] was already released — the newest release
     * tag, not pointing at [headRev], whose `gradle.properties` carries the same `codegenVersion`.
     *
     * That tag is the right comparison base: everything published under this version was published
     * from there, so anything that changed since is not yet on Maven Central. Returns null when the
     * version has never been tagged, which means it is new and there is nothing to compare.
     */
    private fun previousReleaseAtSameVersion(
        headRev: String,
        codegenVersion: String,
    ): ReleaseTag? {
        val headCommit = git("rev-parse", headRev).trim()
        val tags =
            git("tag", "--list", releaseTagPattern, "--merged", headRev, "--sort=-creatordate")
                .lines()
                .map { it.trim() }
                .filter { it.isNotEmpty() }

        if (tags.isEmpty()) {
            logger.info("No tags matching $releaseTagPattern are reachable from $headRev")
            return null
        }

        for (tag in tags) {
            val commit = git("rev-list", "-n", "1", tag).trim()
            // Skip the tag for the release being built: on a release the tag is cut from HEAD, so
            // comparing against it would always report no changes.
            if (commit == headCommit) {
                logger.info("Skipping $tag; it points at $headRev")
                continue
            }
            val taggedVersion = codegenVersionAt(tag)
            if (taggedVersion == codegenVersion) {
                return ReleaseTag(tag, commit)
            }
            logger.info("$tag has codegenVersion $taggedVersion, not $codegenVersion")
        }
        return null
    }

    /** Reads `codegenVersion` out of `gradle.properties` as it was at [rev]. */
    private fun codegenVersionAt(rev: String): String? =
        git("show", "$rev:gradle.properties")
            .lineSequence()
            .map { it.trim() }
            .firstOrNull { it.startsWith("codegenVersion=") }
            ?.substringAfter('=')
            ?.trim()

    /** The published-artifact source paths that changed between [baseRev] and [headRev]. */
    data class SourceChanges(
        val changedPaths: Map<String, List<String>>,
    ) {
        val fileCount: Int get() = changedPaths.values.sumOf { it.size }
    }

    /**
     * The published-artifact source paths that differ between [baseRev] and [headRev].
     *
     * When [headRev] is the working tree (the normal case), this compares the base against the
     * working tree rather than against the HEAD commit, and separately picks up new untracked files.
     * A commit-to-commit diff cannot see uncommitted work, so a developer running this before
     * committing would be told there is nothing to publish while holding exactly the change that
     * makes that false. In CI and in the release pipeline the tree is clean and the two forms agree,
     * so this only adds safety locally.
     */
    private fun changedArtifactSources(
        baseRev: String,
        headRev: String,
    ): SourceChanges {
        val comparingWorkingTree = headRev == WORKING_TREE
        val untracked =
            if (comparingWorkingTree) {
                git("ls-files", "--others", "--exclude-standard")
                    .lines()
                    .map { it.trim() }
                    .filter { it.isNotEmpty() }
            } else {
                emptyList()
            }

        val changed = LinkedHashMap<String, List<String>>()
        for (path in PublishedMavenArtifacts.allSourcePaths) {
            val diffArgs =
                if (comparingWorkingTree) {
                    // No `..HEAD`: this diffs the base against the working tree, staged and not.
                    arrayOf("diff", "--name-only", baseRev, "--", path)
                } else {
                    arrayOf("diff", "--name-only", "$baseRev..$headRev", "--", path)
                }
            val files =
                (
                    git(*diffArgs)
                        .lines()
                        .map { it.trim() }
                        .filter { it.isNotEmpty() } +
                        untracked.filter { it == path || it.startsWith("$path/") }
                ).distinct()
            if (files.isNotEmpty()) {
                changed[path] = files
            }
        }
        return SourceChanges(changed)
    }

    /**
     * Runs git and returns stdout. Any failure is fatal: guessing at the comparison base is how a
     * duplicate version reaches Sonatype, which is the outcome this task exists to prevent.
     */
    private fun git(vararg args: String): String {
        val command = listOf("git") + args
        val process =
            try {
                ProcessBuilder(command)
                    .directory(project.rootDir)
                    .redirectErrorStream(false)
                    .start()
            } catch (e: IOException) {
                throw GradleException("Could not run `${command.joinToString(" ")}`: ${e.message}", e)
            }

        val stdout = process.inputStream.bufferedReader().use { it.readText() }
        val stderr = process.errorStream.bufferedReader().use { it.readText() }
        if (!process.waitFor(60, TimeUnit.SECONDS)) {
            process.destroyForcibly()
            throw GradleException("`${command.joinToString(" ")}` did not finish within 60s")
        }
        if (process.exitValue() != 0) {
            throw GradleException(
                "`${command.joinToString(" ")}` failed with exit code ${process.exitValue()}.\n" +
                    "  stderr: ${stderr.trim()}\n\n" +
                    "This check needs git history and release tags to decide whether Maven Central " +
                    "publishing is needed. A shallow clone without tags is not enough; fetch tags " +
                    "first (`git fetch --tags`).",
            )
        }
        return stdout
    }

    // ---------------------------------------------------------------------------------------------
    // repo1.maven.org
    // ---------------------------------------------------------------------------------------------

    data class PublishState(
        val version: String,
        val present: List<String>,
        val missing: List<String>,
        val inconclusive: List<String> = emptyList(),
    ) {
        val isFullyPublished: Boolean get() = missing.isEmpty() && inconclusive.isEmpty()
        val isPartial: Boolean get() = present.isNotEmpty() && missing.isNotEmpty()

        /** repo1 gave no usable answer for at least one artifact, so decide from git alone. */
        val isInconclusive: Boolean get() = inconclusive.isNotEmpty()
    }

    private fun probePublishState(codegenVersion: String): PublishState {
        val present = mutableListOf<String>()
        val missing = mutableListOf<String>()
        val inconclusive = mutableListOf<String>()
        for (artifactId in PublishedMavenArtifacts.artifactIds) {
            when (isPublished(artifactId, codegenVersion)) {
                true -> present += artifactId
                false -> missing += artifactId
                null -> inconclusive += artifactId
            }
        }
        return PublishState(codegenVersion, present, missing, inconclusive)
    }

    /**
     * HEADs the artifact's POM. Returns null when repo1 gives no conclusive answer.
     *
     * Only a 404 counts as "not published" — treating any other non-200 as absent would let an
     * outage wave a duplicate version through to Sonatype. But it must not throw either: this task
     * runs in `test-codegen`, which gates `Matrix Success`, so a thrown exception here would block
     * every pull request in the repository on repo1's availability. The caller falls back to git.
     */
    private fun isPublished(
        artifactId: String,
        codegenVersion: String,
    ): Boolean? {
        val url = PublishedMavenArtifacts.pomUrl(artifactId, codegenVersion)

        val connection =
            try {
                (URL(url).openConnection() as HttpURLConnection).apply {
                    requestMethod = "HEAD"
                    connectTimeout = connectTimeoutMillis
                    readTimeout = readTimeoutMillis
                    instanceFollowRedirects = false
                }
            } catch (e: IOException) {
                logger.warn("Could not open a connection to $url: ${e.message}")
                return null
            }

        try {
            val responseCode =
                try {
                    connection.responseCode
                } catch (e: IOException) {
                    logger.warn("Could not reach $url: ${e.message}")
                    return null
                }

            logger.info("HEAD $url -> $responseCode")

            return when (responseCode) {
                HttpURLConnection.HTTP_OK -> true
                HttpURLConnection.HTTP_NOT_FOUND -> false
                else -> {
                    logger.warn("Unexpected HTTP $responseCode from $url; treating as inconclusive")
                    null
                }
            }
        } finally {
            connection.disconnect()
        }
    }

    // ---------------------------------------------------------------------------------------------
    // reporting
    // ---------------------------------------------------------------------------------------------

    private fun diagnosticReport(
        codegenVersion: String,
        headRev: String,
        baseTag: ReleaseTag?,
        changes: SourceChanges?,
        publishState: PublishState,
    ): String {
        val head = git("rev-parse", "--short", headRev).trim()
        val lines = mutableListOf<String>()
        lines += "  codegenVersion:     $codegenVersion"
        lines += "  HEAD:               $head ($headRev)"
        lines +=
            if (baseTag == null) {
                "  comparison base:    none — codegenVersion $codegenVersion has not been released before"
            } else {
                "  comparison base:    ${baseTag.name} (${baseTag.commit.take(9)}) — released codegenVersion $codegenVersion"
            }

        lines += "  on Maven Central:   ${publishState.present.size} of ${PublishedMavenArtifacts.artifactIds.size} artifacts"
        if (publishState.present.isNotEmpty()) {
            lines += "    present:          ${publishState.present.joinToString(", ")}"
        }
        if (publishState.missing.isNotEmpty()) {
            lines += "    missing:          ${publishState.missing.joinToString(", ")}"
        }

        when {
            changes == null ->
                lines += "  artifact sources:   not compared (no previous release at this version)"
            changes.changedPaths.isEmpty() ->
                lines += "  artifact sources:   unchanged since ${baseTag?.name} (${PublishedMavenArtifacts.allSourcePaths.size} paths checked)"
            else -> {
                lines += "  artifact sources:   ${changes.fileCount} file(s) changed since ${baseTag?.name}"
                changes.changedPaths.forEach { (path, files) ->
                    lines += "    $path: ${files.size} file(s)"
                    files.take(MAX_FILES_LISTED).forEach { lines += "      $it" }
                    if (files.size > MAX_FILES_LISTED) {
                        lines += "      ... and ${files.size - MAX_FILES_LISTED} more"
                    }
                }
            }
        }
        return lines.joinToString("\n")
    }

    private fun needsBumpMessage(
        codegenVersion: String,
        baseTag: ReleaseTag,
        changes: SourceChanges,
    ): String =
        buildString {
            append(
                "codegenVersion $codegenVersion is already published to Maven Central, but " +
                    "${changes.fileCount} file(s) that feed a published artifact have changed since " +
                    "${baseTag.name} (${baseTag.commit.take(9)}), the release that published it.",
            )
            append("\n\nChanged paths:\n")
            changes.changedPaths.forEach { (path, files) -> append("  $path (${files.size} file(s))\n") }
            append(
                "\nMaven Central coordinates are immutable, so $codegenVersion cannot be " +
                    "republished with these changes. Bump codegenVersion in gradle.properties and " +
                    "release the new version. Published versions are listed at:\n\n  " +
                    PublishedMavenArtifacts.metadataUrl("codegen-core"),
            )
            append(
                "\n\nIf you believe none of these changes affect the published artifacts, correct " +
                    "the path list in buildSrc/src/main/kotlin/PublishedMavenArtifacts.kt rather " +
                    "than bypassing this check.",
            )
        }

    private fun partialPublicationMessage(
        codegenVersion: String,
        publishState: PublishState,
    ): String =
        buildString {
            append("codegenVersion $codegenVersion is PARTIALLY published to Maven Central.\n\n")
            append("  present (${publishState.present.size}): ${publishState.present.joinToString(", ")}\n")
            append("  missing (${publishState.missing.size}): ${publishState.missing.joinToString(", ")}\n")
            append(
                "\nMaven Central coordinates are immutable, so the artifacts that are present " +
                    "cannot be replaced and this version can never be completed. Bump " +
                    "codegenVersion in gradle.properties and release the new version.",
            )
            append(
                "\n\nTwo states look like this but are not broken. First, repo1.maven.org lags the " +
                    "Sonatype publishing dashboard, so a release that finished in the last hour can " +
                    "look partial while it is still syncing — check " +
                    "https://central.sonatype.com/publishing/deployments. Second, an artifact added " +
                    "to PublishedMavenArtifacts more recently than this version legitimately has no " +
                    "release at it; that only happens when checking a historical version.",
            )
        }

    /**
     * Records that `codegenVersion` needs bumping, without failing the build.
     *
     * This task runs in `test-codegen`, which gates `Matrix Success`, so throwing here would block
     * the merge. Instead the finding is written where CI can pick it up and surface it as a pull
     * request comment, and as a workflow annotation on fork runs where commenting is not permitted.
     *
     * The tradeoff is deliberate and worth stating: a comment informs, it does not prevent. If it is
     * ignored, the release publishes jars whose contents no longer match the codegen that built
     * them, and the Maven publisher cannot catch that because it only sees repo1.
     */
    private fun reportBumpNeeded(message: String) {
        val outputDir = project.layout.buildDirectory.dir("maven-central").get().asFile
        outputDir.mkdirs()
        File(outputDir, "bump-needed.md").writeText(message.trimEnd() + "\n")
        logger.warn("==> BUMP NEEDED\n\n$message")
    }

    companion object {
        /**
         * Sentinel meaning "compare against the working tree". It is also a valid git revision, so
         * `rev-parse` and `tag --merged` still work with it; only the diff form branches on it.
         */
        private const val WORKING_TREE = "HEAD"

        private const val MAX_FILES_LISTED = 10
    }
}
