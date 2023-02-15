/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.transformers

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.core.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.server.smithy.traits.ConstraintViolationRustBoxTrait

object RecursiveConstraintViolationBoxer {
    /**
     * Transform a model which may contain recursive shapes into a model annotated with [ConstraintViolationRustBoxTrait].
     *
     * See [RecursiveShapeBoxer] for how the tagging algorithm works.
     *
     * The constraint violation graph needs to box types in recursive paths more often. Since we don't collect
     * constraint violations (yet, see [0]), the constraint violation graph never holds `Vec<T>`s or `HashMap<K, V>`s,
     * only simple types. Indeed, the following simple recursive model:
     *
     * ```smithy
     * union Recursive {
     *     list: List
     * }
     *
     * @length(min: 69)
     * list List {
     *     member: Recursive
     * }
     * ```
     *
     * has a cycle that goes through a list shape, so no shapes in it need boxing in the regular shape graph. However,
     * the constraint violation graph is infinitely recursive if we don't introduce boxing somewhere:
     *
     * ```rust
     * pub mod model {
     *     pub mod list {
     *         pub enum ConstraintViolation {
     *             Length(usize),
     *             Member(
     *                 usize,
     *                 crate::model::recursive::ConstraintViolation,
     *             ),
     *         }
     *     }
     *
     *     pub mod recursive {
     *         pub enum ConstraintViolation {
     *             List(crate::model::list::ConstraintViolation),
     *         }
     *     }
     * }
     * ```
     *
     * So what we do to fix this is to configure the `RecursiveShapeBoxer` model transform so that the "cycles through
     * lists and maps introduce indirection" assumption can be lifted. This allows this model transform to tag member
     * shapes along recursive paths with a new trait, `ConstraintViolationRustBoxTrait`, that the constraint violation
     * type generation then utilizes to ensure that no infinitely recursive constraint violation types get generated.
     * Places where constraint violations are handled (like where unconstrained types are converted to constrained
     * types) must account for the scenario where they now are or need to be boxed.
     *
     * [0] https://github.com/awslabs/smithy-rs/pull/2040
     */
    fun transform(model: Model): Model = RecursiveShapeBoxer(
        containsIndirectionPredicate = ::constraintViolationLoopContainsIndirection,
        boxShapeFn = ::addConstraintViolationRustBoxTrait,
    ).transform(model)

    private fun constraintViolationLoopContainsIndirection(loop: Collection<Shape>): Boolean =
        loop.find { it.hasTrait<ConstraintViolationRustBoxTrait>() } != null

    private fun addConstraintViolationRustBoxTrait(memberShape: MemberShape): MemberShape =
        memberShape.toBuilder().addTrait(ConstraintViolationRustBoxTrait()).build()
}
