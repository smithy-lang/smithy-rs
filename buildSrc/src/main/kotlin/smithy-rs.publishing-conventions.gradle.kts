plugins {
    `maven-publish`
    signing
}
// FIXME(publishing): create a real "javadoc" JAR from Dokka output
val javadocJar = tasks.register<Jar>("emptyJar") {
    archiveClassifier.set("javadoc")
    destinationDirectory.set(layout.buildDirectory.dir("libs"))
    from()
}

publishing {
    publications {
        create<MavenPublication>("codegen") {
            from(components["java"])
            artifact(javadocJar)

            afterEvaluate {
                pom {
                    name.set(project.ext["displayName"].toString())
                    description.set(project.description)
                    url.set("https://github.com/smithy-lang/smithy-rs")
                    licenses {
                        license {
                            name.set("Apache License 2.0")
                            url.set("http://www.apache.org/licenses/LICENSE-2.0.txt")
                            distribution.set("repo")
                        }
                    }
                    developers {
                        developer {
                            id.set("smithy-rs")
                            name.set("Smithy Rust")
                            organization.set("Amazon Web Services")
                            organizationUrl.set("https://aws.amazon.com")
                            roles.add("developer")
                        }
                    }
                    scm {
                        url.set("https://github.com/smithy-lang/smithy-rs.git")
                    }
                }
            }


        }
    }
    repositories {
        maven {
            name = "localStaging"
            url = uri(stagingDir())
        }
    }

    // Don't sign the artifacts if we didn't get a key and password to use.
    if (project.hasProperty("signingKey") && project.hasProperty("signingPassword")) {
        signing {
            useInMemoryPgpKeys(
                project.property("signingKey").toString(),
                project.property("signingPassword").toString()
            )
            sign(publishing.publications["codegen"])
        }
    }
}
