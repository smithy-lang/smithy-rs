/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::shape_id::ShapeId;

/// Models the [Smithy Operation shape].
///
/// [Smithy Operation shape]: https://awslabs.github.io/smithy/1.0/spec/core/model.html#operation
pub trait OperationShape {
    /// The ID of the operation.
    const ID: ShapeId;
}
