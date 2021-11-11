import java.util.Properties

plugins {
    `kotlin-dsl`
}
repositories {
    maven("https://plugins.gradle.org/m2")
}

// Load properties manually to avoid hard coding smithy version
val props = Properties().apply {
    file("../gradle.properties").inputStream().use { load(it) }
}

val smithyVersion = props["smithyVersion"]

buildscript {
    repositories {
        mavenCentral()
    }
}

dependencies {
    api("software.amazon.smithy:smithy-codegen-core:$smithyVersion")
    implementation("software.amazon.smithy:smithy-utils:$smithyVersion")
    implementation("software.amazon.smithy:smithy-protocol-test-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-iam-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-cloudformation-traits:$smithyVersion")
    implementation(gradleApi())
}
