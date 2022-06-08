/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.gradle.api.Project
import java.io.File
import java.util.Properties

class PropertyRetriever(private val rootProject: Project, private val project: Project) {
    /** Get a project property by name if it exists (including from local.properties) */
    fun get(name: String): String? {
        if (project.hasProperty(name)) {
            return project.properties[name].toString()
        }

        val localProperties = Properties()
        val propertiesFile: File = rootProject.file("local.properties")
        if (propertiesFile.exists()) {
            propertiesFile.inputStream().use { localProperties.load(it) }

            if (localProperties.containsKey(name)) {
                return localProperties[name].toString()
            }
        }
        return null
    }
}
