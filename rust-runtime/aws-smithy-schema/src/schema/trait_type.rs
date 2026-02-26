/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::ShapeId;
use std::any::Any;
use std::fmt;

/// Trait representing a Smithy trait at runtime.
///
/// Traits provide additional metadata about shapes that affect serialization,
/// validation, and other behaviors.
pub trait Trait: Any + Send + Sync + fmt::Debug {
    /// Returns the Shape ID of this trait.
    fn trait_id(&self) -> &ShapeId;

    /// Returns this trait as `&dyn Any` for downcasting.
    fn as_any(&self) -> &dyn Any;
}
