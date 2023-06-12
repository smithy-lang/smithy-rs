/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::shape_id::ShapeId;

/// Models the [Smithy Service shape].
///
/// [Smithy Service shape]: https://smithy.io/2.0/spec/service-types.html
pub trait ServiceShape {
    /// The [`ShapeId`] of the service.
    const ID: ShapeId;

    /// The [Protocol] applied to this service.
    ///
    /// [Protocol]: https://smithy.io/2.0/spec/protocol-traits.html
    type Protocol;

    /// An enumeration of all operations contained in this service.
    type Operations;
}
