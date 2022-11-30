/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.transformers

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.core.util.orNull

/**
 * The Amazon Elastic Block Store (Amazon EBS) model is one model that we generate in CI.
 * Unfortunately, it defines its own `ValidationException` shape, that conflicts with
 * `smithy.framework#ValidationException` [0].
 *
 * So this is a model that a service owner would generate when "disabling default validation": in such a code generation
 * mode, the service owner is responsible for mapping an operation input-level constraint violation into a modeled
 * operation error. This mode, as well as what the end goal for validation exception responses looks like, is described
 * in more detail in [1]. We don't support this mode yet.
 *
 * So this transformer simply removes the EBB model's `ValidationException`. A subsequent model transformer,
 * [AttachValidationExceptionToConstrainedOperationInputsInAllowList], ensures that it is replaced by
 * `smithy.framework#ValidationException`.
 *
 * [0]: https://github.com/awslabs/smithy-rs/blob/274adf155042cde49251a0e6b8842d6f56cd5b6d/codegen-core/common-test-models/ebs.json#L1270-L1288
 * [1]: https://github.com/awslabs/smithy-rs/pull/1199#discussion_r809424783
 *
 * TODO(https://github.com/awslabs/smithy-rs/issues/1401): This transformer will go away once we implement
 *  `disableDefaultValidation` set to `true`, allowing service owners to map from constraint violations to operation errors.
 */
object RemoveEbsModelValidationException {
    fun transform(model: Model): Model {
        val shapeToRemove = model.getShape(ShapeId.from("com.amazonaws.ebs#ValidationException")).orNull()
        return ModelTransformer.create().removeShapes(model, listOfNotNull(shapeToRemove))
    }
}
