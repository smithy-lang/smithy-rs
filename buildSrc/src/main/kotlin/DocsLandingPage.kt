import org.gradle.api.Project
import software.amazon.smithy.utils.CodeWriter
import java.io.File

/**
 * Generate a basic docs landing page into [outputDir]
 *
 * The generated docs will include links to crates.io, docs.rs and GitHub examples for all generated services. The generated docs will
 * be written to `docs.md` in the provided [outputDir].
 */
fun Project.docsLandingPage(awsServices: List<AwsService>, outputDir: File) {
    val project = this
    val writer = CodeWriter()
    with(writer) {
        write("# AWS SDK for Rust")
        write(
            """The AWS SDK for Rust contains one crate for each AWS service, as well as ${docsRs("aws-config")},
            |a crate implementing configuration loading such as credential providers.""".trimMargin()
        )

        writer.write("## AWS Services")
        /* generate a basic markdown table */
        writer.write("| Service | [docs.rs](https://docs.rs) | [crates.io](https://crates.io) | [Usage Examples](https://github.com/awslabs/aws-sdk-rust/tree/main/examples/) |")
        writer.write("| ------- | ------- | --------- | ------ |")
        awsServices.forEach {
            writer.write(
                "| ${it.humanName} | ${docsRs(it)} | ${cratesIo(it)} | ${
                examples(
                    it,
                    project
                )
                }"
            )
        }
    }
    outputDir.resolve("docs.md").writeText(writer.toString())
}

/**
 * Generate a link to the examples for a given service
 */
private fun examples(service: AwsService, project: Project) = if (with(service) { project.examples() }) {
    "[Link](https://github.com/awslabs/aws-sdk-rust/tree/main/examples/${service.module})"
} else {
    "None yet!"
}

/**
 * Generate a link to the docs
 */
private fun docsRs(service: AwsService) = docsRs(service.crate())
private fun docsRs(crate: String) = "[$crate](https://docs.rs/$crate)"
private fun cratesIo(service: AwsService) = "[${service.crate()}](https://crates.io/crates/${service.crate()})"
