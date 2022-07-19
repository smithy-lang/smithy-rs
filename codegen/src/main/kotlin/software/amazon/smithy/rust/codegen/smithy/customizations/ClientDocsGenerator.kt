/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.customizations

import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.containerDocs
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection

class ClientDocsGenerator : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable {
        return when (section) {
            is LibRsSection.ModuleDocumentation -> if (section.subsection == LibRsSection.CrateOrganization) {
                crateLayout()
            } else emptySection
            else -> emptySection
        }
    }

    private fun crateLayout(): Writable = writable {
        containerDocs(
            """
        The entry point for most customers will be [`Client`]. [`Client`] exposes one method for each API offered
        by the service.

        Some APIs require complex or nested arguments. These exist in [`model`](crate::model).

        Lastly, errors that can be returned by the service are contained within [`error`]. [`Error`] defines a meta
        error encompassing all possible errors that can be returned by the service.

        The other modules within this crate are not required for normal usage.
        """.trimEnd()
        )
    }
}
