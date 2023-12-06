/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.gradle.api.tasks.testing.logging.TestExceptionFormat
import java.util.Properties

plugins {
    `kotlin-dsl`
    jacoco
}
repositories {
    mavenCentral()
    google()
    /* mavenLocal() */
}

// Load properties manually to avoid hard coding smithy version
val props = Properties().apply {
    file("../gradle.properties").inputStream().use { load(it) }
}

val smithyVersion = props["smithyVersion"]

dependencies {
    api("software.amazon.smithy:smithy-codegen-core:$smithyVersion")
    implementation("software.amazon.smithy:smithy-utils:$smithyVersion")
    implementation("software.amazon.smithy:smithy-protocol-test-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-iam-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-cloudformation-traits:$smithyVersion")
    implementation(gradleApi())
    implementation("com.moandjiezana.toml:toml4j:0.7.2")
    testImplementation("org.junit.jupiter:junit-jupiter:5.6.1")

    constraints {
        implementation("com.google.code.gson:gson:2.8.9") {
            because("transitive dependency of toml4j has vulnerabilities; this upgrades it to the patched version")
        }
    }
}

tasks.test {
    useJUnitPlatform()
    testLogging {
        events("passed", "skipped", "failed")
        exceptionFormat = TestExceptionFormat.FULL
        showCauses = true
        showExceptions = true
        showStackTraces = true
        showStandardStreams = true
    }
}

// Configure jacoco (code coverage) to generate an HTML report
tasks.jacocoTestReport {
    reports {
        xml.required.set(false)
        csv.required.set(false)
        html.outputLocation.set(layout.buildDirectory.dir("reports/jacoco"))
    }
}

// Always run the jacoco test report after testing.
tasks["test"].finalizedBy(tasks["jacocoTestReport"])
