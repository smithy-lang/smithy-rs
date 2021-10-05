/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.rustlang

data class RustModule(val name: String, val rustMetadata: RustMetadata) {
    fun render(writer: RustWriter) {
        rustMetadata.render(writer)
        writer.write("mod $name;")
    }

    companion object {
        fun default(name: String, public: Boolean): RustModule {
            // TODO: figure out how to enable this, but only for real services (protocol tests don't have documentation)
            /*val attributes = if (public) {
                listOf(Custom("deny(missing_docs)"))
            } else {
                listOf()
            }*/
            return RustModule(name, RustMetadata(public = public))
        }

        fun public(name: String): RustModule = default(name, public = true)
        fun private(name: String): RustModule = default(name, public = false)

        val Config = public("config")
        val Error = public("error")
    }
}
