/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.jsoup.Jsoup
import org.jsoup.nodes.Element
import org.jsoup.nodes.TextNode
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.containerDocsTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.rawTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.ManifestCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.generators.ModuleDocSection
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.serviceNameOrDefault
import java.util.logging.Logger

// Use a sigil that should always be unique in the text to fix line breaks and spaces
// since Jsoup doesn't preserve whitespace at all.
private const val LINE_BREAK_SIGIL = "[[smithy-rs-br]]"
private const val SPACE_SIGIL = "[[smithy-rs-nbsp]]"

/**
 * Generates a README.md and top-level crate documentation for each service crate for display on crates.io and docs.rs.
 */
class AwsCrateDocsDecorator : ClientCodegenDecorator {
    override val name: String = "AwsReadmeDecorator"
    override val order: Byte = 0

    override fun crateManifestCustomizations(codegenContext: ClientCodegenContext): ManifestCustomizations =
        if (generateReadme(codegenContext)) {
            mapOf("package" to mapOf("readme" to "README.md"))
        } else {
            emptyMap()
        }

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> =
        baseCustomizations +
            listOf(
                object : LibRsCustomization() {
                    override fun section(section: LibRsSection): Writable =
                        when {
                            section is LibRsSection.ModuleDoc && section.subsection is ModuleDocSection.ServiceDocs ->
                                writable {
                                    // Include README contents in crate docs if they are to be generated
                                    if (generateReadme(codegenContext)) {
                                        AwsCrateDocGenerator(codegenContext).generateCrateDocComment()(this)
                                    }
                                }

                            else -> emptySection
                        }
                },
            )

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        if (generateReadme(codegenContext)) {
            AwsCrateDocGenerator(codegenContext).generateReadme(rustCrate)
        }
    }

    override fun clientConstructionDocs(
        codegenContext: ClientCodegenContext,
        baseDocs: Writable,
    ): Writable =
        writable {
            val serviceName = codegenContext.serviceShape.serviceNameOrDefault("the service")
            docs("Client for calling $serviceName.")
            if (generateReadme(codegenContext)) {
                AwsDocs.clientConstructionDocs(codegenContext)(this)
            }
        }

    private fun generateReadme(codegenContext: ClientCodegenContext) =
        SdkSettings.from(codegenContext.settings).generateReadme
}

sealed class DocSection(name: String) : AdHocSection(name) {
    data class CreateClient(val crateName: String, val clientName: String = "client", val indent: String) : DocSection("CustomExample")
}

internal class AwsCrateDocGenerator(private val codegenContext: ClientCodegenContext) {
    private val logger: Logger = Logger.getLogger(javaClass.name)
    private val awsConfigVersion by lazy {
        SdkSettings.from(codegenContext.settings).awsConfigVersion
            ?: throw IllegalStateException("missing `awsConfigVersion` codegen setting")
    }

    private fun RustWriter.template(
        asComments: Boolean,
        text: String,
        vararg args: Pair<String, Any>,
    ) = when (asComments) {
        true -> containerDocsTemplate(text, *args)
        else -> rawTemplate(text + "\n", *args)
    }

    internal fun docText(
        includeHeader: Boolean,
        includeLicense: Boolean,
        asComments: Boolean,
    ): Writable =
        writable {
            val moduleVersion = codegenContext.settings.moduleVersion
            check(moduleVersion.isNotEmpty() && moduleVersion[0].isDigit())

            val moduleName = codegenContext.settings.moduleName
            val description =
                normalizeDescription(
                    codegenContext.moduleName,
                    codegenContext.settings.getService(codegenContext.model).getTrait<DocumentationTrait>()?.value ?: "",
                )
            val snakeCaseModuleName = moduleName.replace('-', '_')
            val shortModuleName = moduleName.removePrefix("aws-sdk-")

            if (includeHeader) {
                template(asComments, escape("# $moduleName\n"))
            }

            if (description.isNotBlank()) {
                template(asComments, escape("$description\n"))
            }

            val compileExample = AwsDocs.canRelyOnAwsConfig(codegenContext)
            val exampleMode = if (compileExample) "no_run" else "ignore"
            template(
                asComments,
                """
                #### Getting Started

                > Examples are available for many services and operations, check out the
                > [examples folder in GitHub](https://github.com/awslabs/aws-sdk-rust/tree/main/examples).

                The SDK provides one crate per AWS service. You must add [Tokio](https://crates.io/crates/tokio)
                as a dependency within your Rust project to execute asynchronous code. To add `$moduleName` to
                your project, add the following to your **Cargo.toml** file:

                ```toml
                [dependencies]
                aws-config = { version = "$awsConfigVersion", features = ["behavior-version-latest"] }
                $moduleName = "$moduleVersion"
                tokio = { version = "1", features = ["full"] }
                ```

                Then in code, a client can be created with the following:

                ```rust,$exampleMode
                use $snakeCaseModuleName as $shortModuleName;

                ##[#{tokio}::main]
                async fn main() -> Result<(), $shortModuleName::Error> {
                    #{constructClient}

                    // ... make some calls with the client

                    Ok(())
                }
                ```

                See the [client documentation](https://docs.rs/$moduleName/latest/$snakeCaseModuleName/client/struct.Client.html)
                for information on what calls can be made, and the inputs and outputs for each of those calls.${"\n"}
                """.trimIndent().trimStart(),
                "tokio" to CargoDependency.Tokio.toDevDependency().toType(),
                "aws_config" to
                    when (compileExample) {
                        true -> AwsCargoDependency.awsConfig(codegenContext.runtimeConfig).toDevDependency().toType()
                        else -> writable { rust("aws_config") }
                    },
                "constructClient" to AwsDocs.constructClient(codegenContext, indent = "    "),
            )

            template(
                asComments,
                """
                #### Using the SDK

                Until the SDK is released, we will be adding information about using the SDK to the
                [Developer Guide](https://docs.aws.amazon.com/sdk-for-rust/latest/dg/welcome.html). Feel free to suggest
                additional sections for the guide by opening an issue and describing what you are trying to do.${"\n"}
                """.trimIndent(),
            )

            template(
                asComments,
                """
                #### Getting Help

                * [GitHub discussions](https://github.com/awslabs/aws-sdk-rust/discussions) - For ideas, RFCs & general questions
                * [GitHub issues](https://github.com/awslabs/aws-sdk-rust/issues/new/choose) - For bug reports & feature requests
                * [Generated Docs (latest version)](https://awslabs.github.io/aws-sdk-rust/)
                * [Usage examples](https://github.com/awslabs/aws-sdk-rust/tree/main/examples)${"\n"}
                """.trimIndent(),
            )

            if (includeLicense) {
                template(
                    asComments,
                    """
                    #### License

                    This project is licensed under the Apache-2.0 License.
                    """.trimIndent(),
                )
            }
        }

    internal fun generateCrateDocComment(): Writable =
        docText(includeHeader = false, includeLicense = false, asComments = true)

    internal fun generateReadme(rustCrate: RustCrate) =
        rustCrate.withFile("README.md") {
            docText(includeHeader = true, includeLicense = true, asComments = false)(this)
        }

    /**
     * Strips HTML from the description and makes it human-readable Markdown.
     */
    internal fun normalizeDescription(
        moduleName: String,
        input: String,
    ): String {
        val doc = Jsoup.parse(input)
        doc.body().apply {
            // The order of operations here is important:
            stripUndesiredNodes() // Remove `<fullname>`, blank whitespace nodes, etc
            normalizeInlineStyles() // Convert bold/italics tags to Markdown equivalents
            normalizeAnchors() // Convert anchor tags into Markdown links
            normalizeBreaks() // Convert `<br>` tags into newlines
            normalizeLists() // Convert HTML lists into Markdown lists
            normalizeDescriptionLists() // Converts HTML <dl> description lists into Markdown
            normalizeParagraphs() // Replace paragraph tags into blocks of text separated by newlines
            warnOnUnrecognizedElements(moduleName) // Log a warning if we missed something
        }
        return doc.body().text()
            .replace(LINE_BREAK_SIGIL, "\n")
            .replace(SPACE_SIGIL, " ")
            .normalizeLineWhitespace()
    }

    private fun Element.stripUndesiredNodes() {
        // Remove the `<fullname>` tag
        getElementsByTag("fullname").forEach { it.remove() }
        // Unwrap `<important>` tags
        getElementsByTag("important").forEach { it.changeInto("span") }
        // Remove the `<note>` tag
        getElementsByTag("note").forEach {
            if (it.children().isEmpty()) {
                throw IllegalStateException("<note> tag unexpectedly had children")
            }
            it.remove()
        }

        // Eliminate empty whitespace
        textNodes().forEach { text ->
            if (text.isBlank) {
                text.remove()
            }
        }
    }

    private fun Element.changeInto(tagName: String) {
        replaceWith(Element(tagName).also { elem -> elem.appendChildren(childNodesCopy()) })
    }

    private fun Element.normalizeInlineStyles() {
        getElementsByTag("b").forEach { normalizeInlineStyleTag("__", it) }
        getElementsByTag("i").forEach { normalizeInlineStyleTag("_", it) }
    }

    private fun normalizeInlineStyleTag(
        surround: String,
        tag: Element,
    ) {
        tag.replaceWith(
            Element("span").also { span ->
                span.append(surround)
                span.appendChildren(tag.childNodesCopy())
                span.append(surround)
            },
        )
    }

    private fun Element.normalizeAnchors() {
        for (anchor in getElementsByTag("a")) {
            val text = anchor.text()
            val link = anchor.attr("href")
            anchor.replaceWith(
                TextNode(
                    if (link.isNotBlank()) {
                        "[$text]($link)"
                    } else {
                        text
                    },
                ),
            )
        }
    }

    private fun Element.normalizeBreaks() {
        getElementsByTag("br").forEach { lineBreak -> lineBreak.replaceWith(TextNode(LINE_BREAK_SIGIL)) }
    }

    private fun Element.isList(): Boolean = tagName() == "ul" || tagName() == "ol"

    private fun Element.normalizeLists() {
        (getElementsByTag("ul") + getElementsByTag("ol"))
            // Only operate on lists that are top-level (are not nested within other lists)
            .filter { list -> list.parents().none { it.isList() } }
            .forEach { list -> list.normalizeList() }
    }

    private fun Element.normalizeList(indent: Int = 1) {
        // First, replace nested lists
        for (child in children().filter { tag -> tag.tagName() == "li" }) {
            for (itemChild in child.children()) {
                if (itemChild.isList()) {
                    itemChild.normalizeList(indent + 1)
                }
            }
        }
        // Then format the list items down to Markdown
        val result = StringBuilder(if (indent == 1) "" else LINE_BREAK_SIGIL)
        val prefix = if (tagName() == "ul") "- " else "1. "
        val indentText = SPACE_SIGIL.repeat(indent * 2)
        for (child in children()) {
            result.append("$indentText$prefix${child.text().trim()}$LINE_BREAK_SIGIL")
        }
        replaceWith(TextNode(result.toString()))
    }

    private fun Element.normalizeDescriptionLists() {
        getElementsByTag("dl").forEach { list -> list.normalizeDescriptionList() }
    }

    private fun Element.normalizeDescriptionList() {
        getElementsByTag("dt").forEach { dt ->
            dt.text("${LINE_BREAK_SIGIL}__${dt.text()}__$LINE_BREAK_SIGIL")
            dt.changeInto("span")
        }
        getElementsByTag("dd").forEach { dd -> dd.changeInto("p") }
        appendChild(TextNode(LINE_BREAK_SIGIL))
        changeInto("span")
    }

    private fun Element.normalizeParagraphs() {
        getElementsByTag("p").forEach { paragraph ->
            paragraph.replaceWith(TextNode(LINE_BREAK_SIGIL + paragraph.text() + LINE_BREAK_SIGIL))
        }
    }

    private fun Element.warnOnUnrecognizedElements(moduleName: String) {
        allElements
            .filter { elem ->
                // body is always present
                elem.tagName() != "body" &&
                    // we replace certain elements with span, so these are fine
                    elem.tagName() != "span"
            }
            .map { elem -> elem.tagName() }.toSortedSet().joinToString(", ")
            .let { tags ->
                if (tags.isNotEmpty()) {
                    logger.warning { "[$moduleName] Unrecognized HTML tags encountered when normalizing text: $tags" }
                }
            }
    }

    private fun String.normalizeLineWhitespace(): String =
        // Convert sigils back into whitespace
        replace(LINE_BREAK_SIGIL, "\n")
            .replace(SPACE_SIGIL, " ")
            // Replace long runs of linebreaks with just two line breaks
            .replace(Regex("\n\n\n+"), "\n\n")
            // Remove trailing whitespace from each line
            .replace(Regex("[ \t]+\n"), "\n")
            // Remove leading whitespace from each line when it's not a list item
            .replace(Regex("\n[ \t]+([^ \t\\-1])"), "\n$1")
            // Chop off leading newlines
            .replace(Regex("^\n+"), "")
            // Chop off trailing newlines
            .replace(Regex("\n+$"), "")
}
