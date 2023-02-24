/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.util

import org.jsoup.Jsoup
import org.jsoup.nodes.Document
import org.jsoup.nodes.Element
import org.jsoup.nodes.Node
import org.jsoup.nodes.TextNode
import org.jsoup.select.NodeTraversor
import org.jsoup.select.NodeVisitor

object DocManipulator {
    /**
     * Truncates the given documentation after the first '.' encountered outside a `<code>` tag.
     */
    fun truncateDocsAfterFirstPeriod(input: String): String {
        val original = Jsoup.parse(input)
        val truncated = Document.createShell(original.baseUri())

        NodeTraversor.traverse(
            object : NodeVisitor {
                private var currentElem: Element = truncated.body()
                private var done: Boolean = false

                override fun head(node: Node, depth: Int) {
                    if (depth == 0) {
                        return // skip the body tag
                    }
                    when (node) {
                        is Element -> {
                            if (!done) {
                                val parent = currentElem
                                currentElem = Element(node.tag(), node.baseUri(), node.attributes())
                                parent.appendChild(currentElem)
                            }
                        }
                        is TextNode -> {
                            if (!done) {
                                val newText = if (currentElem.tagName() != "code") {
                                    val maybeTruncated = node.text().substringBefore('.')
                                    if (maybeTruncated != node.text()) {
                                        done = true
                                        "$maybeTruncated."
                                    } else {
                                        maybeTruncated
                                    }
                                } else {
                                    node.text()
                                }
                                currentElem.appendText(newText)
                            }
                        }
                    }
                }

                override fun tail(node: Node, depth: Int) {
                    if (depth > 0 && !done && node is Element) {
                        currentElem = currentElem.parent()!!
                    }
                }
            },
            original.body(),
        )
        truncated.outputSettings().prettyPrint(false)
        return truncated.body().html()
    }
}
