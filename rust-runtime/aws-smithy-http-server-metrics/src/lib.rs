/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_cfg))]
/* End of automatically managed default lints */
#![allow(missing_docs)]

pub mod default;
pub mod extension;
pub mod layer;
pub mod plugin;
pub mod service;
pub mod traits;
pub mod types;

pub use layer::builder::MetricsLayerBuilder;
pub use layer::MetricsLayer;

pub use aws_smithy_http_server_metrics_macro::smithy_metrics;
