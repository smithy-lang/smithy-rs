/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::{HttpMarker, Plugin};

/// Adapts a serde-independent HTTP plugin written before serialization modes.
///
/// Delegates to its original `Plugin<Ser, Op, T>` implementation in either mode.
/// This does not remove the inner plugin's bounds: plugins that serialize errors
/// need to adopt [`IntoResponseWithRequest`](crate::response::IntoResponseWithRequest).
#[derive(Clone, Debug)]
pub struct LegacyHttpPlugin<Pl>(Pl);

impl<Pl> LegacyHttpPlugin<Pl> {
    /// Wraps an existing plugin.
    pub fn new(plugin: Pl) -> Self {
        Self(plugin)
    }
}

impl<Ser, Op, T, Pl, SerdeMode> Plugin<Ser, Op, T, SerdeMode> for LegacyHttpPlugin<Pl>
where
    Pl: Plugin<Ser, Op, T>,
{
    type Output = Pl::Output;

    fn apply(&self, input: T) -> Self::Output {
        Plugin::<Ser, Op, T>::apply(&self.0, input)
    }
}

impl<Pl: HttpMarker> HttpMarker for LegacyHttpPlugin<Pl> {}
