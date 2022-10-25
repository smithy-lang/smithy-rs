/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::Plugin;

/// A [`Plugin`] that maps an `input` [`Operation`] to itself.
pub struct IdentityPlugin;

impl<P, Op, ModelLayer, HttpLayer> Plugin<P, Op, ModelLayer, HttpLayer> for IdentityPlugin {
    type ModelLayer = ModelLayer;
    type HttpLayer = HttpLayer;

    fn map(&self, model_layer: ModelLayer, http_layer: HttpLayer) -> (ModelLayer, HttpLayer) {
        (model_layer, http_layer)
    }
}
