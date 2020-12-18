/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.smithy.RuntimeType

class ServiceConfigGenerator(serviceShape: ServiceShape) {
    // TODO: stub
    fun render(writer: RustWriter) {
        writer.write("pub use #T::Config;", RuntimeType.Config)
    }
}
