/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Results {
    pub(crate) product_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sdk_version: Option<String>,
    pub(crate) commit_id: String,
    pub(crate) results: Vec<Result>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Result {
    pub(crate) name: String,
    pub(crate) description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) publish_to_cloudwatch: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) dimensions: Option<Vec<Dimension>>,
    pub(crate) date: u64,
    pub(crate) measurements: Vec<f64>,
    pub(crate) unit: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Dimension {
    pub(crate) name: String,
    pub(crate) value: String,
}
