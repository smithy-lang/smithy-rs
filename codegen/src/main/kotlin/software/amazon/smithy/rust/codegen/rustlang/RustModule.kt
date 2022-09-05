/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.rustlang

data class RustModule(val name: String, val rustMetadata: RustMetadata, val documentation: String?, val hidden: Boolean?) {
    fun render(writer: RustWriter) {
        if (hidden == true) {
            writer.write("##[doc(hidden)]")
        }
        documentation?.let { docs -> writer.docs(docs) }
        rustMetadata.render(writer)

        writer.write("mod $name;")
    }

    companion object {
        fun default(name: String, visibility: Visibility, documentation: String? = null, hidden: Boolean? = false): RustModule {
            return RustModule(name, RustMetadata(visibility = visibility), documentation, hidden)
        }

        fun public(name: String, documentation: String? = null, hidden: Boolean? = false): RustModule =
            default(name, visibility = Visibility.PUBLIC, documentation, hidden)

        fun private(name: String, documentation: String? = null, hidden: Boolean? = false): RustModule =
            default(name, visibility = Visibility.PRIVATE, documentation, hidden)

        val Config = public("config", documentation = "Configuration for the service.")
        val Error = public("error", documentation = "Errors that can occur when calling the service.")
    }
}
