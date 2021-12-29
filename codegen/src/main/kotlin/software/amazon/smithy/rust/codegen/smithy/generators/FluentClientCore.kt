/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.docs
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock

class FluentClientCore(private val model: Model) {
    fun RustWriter.renderVecHelper(member: MemberShape, memberName: String, coreType: RustType.Vec) {
        docs("Appends an item to `${member.memberName}`.")
        rust("///")
        docs("To override the contents of this collection use [`${member.setterName()}`](Self::${member.setterName()}).")
        rust("///")
        documentShape(member, model)
        val (input_param, input_usage) = coreType.formattedMember()

        rustBlock("pub fn $memberName(mut self, $input_param) -> Self") {
            rust(
                """
                self.inner = self.inner.$memberName($input_usage);
                self
                """
            )
        }
    }

    fun RustWriter.renderMapHelper(member: MemberShape, memberName: String, coreType: RustType.HashMap) {
        docs("Adds a key-value pair to `${member.memberName}`.")
        rust("///")
        docs("To override the contents of this collection use [`${member.setterName()}`](Self::${member.setterName()}).")
        rust("///")
        documentShape(member, model)
        val (k_param, k_usage) = coreType.formattedKey()
        val (v_param, v_usage) = coreType.formattedMember()

        rustBlock("pub fn $memberName(mut self, $k_param, $v_param) -> Self") {
            rust(
                """
                self.inner = self.inner.$memberName($k_usage, $v_usage);
                self
                """
            )
        }
    }
}
