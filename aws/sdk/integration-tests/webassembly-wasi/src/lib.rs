/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

mod adapter;
mod default_config;
mod list_buckets;

pub async fn test() {
    let response = crate::list_buckets::s3_list_buckets().await;
    println!(
        "{:#?}",
        response
            .buckets()
            .unwrap_or_default()
            .iter()
            .map(|b| b.name().unwrap())
            .collect::<Vec<_>>()
    );
}
