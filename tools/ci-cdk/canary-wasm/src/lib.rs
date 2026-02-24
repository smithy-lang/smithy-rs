/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::Region;
use aws_sdk_s3 as s3;
use aws_smithy_async::rt::sleep::TokioSleep;
use aws_smithy_wasm::wasi::WasiHttpClientBuilder;

//Generates the Rust bindings from the wit file
wit_bindgen::generate!({
    world: "canary-world",
});

export!(Component);

struct Component;

impl exports::aws::component::canary_interface::Guest for Component {
    fn run_canary() -> Result<Vec<String>, String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("Failed to generate runtime");
        let res = rt.block_on(run_canary())?;
        Ok(res)
    }
}

async fn run_canary() -> Result<Vec<String>, String> {
    let http_client = WasiHttpClientBuilder::new().build();
    let sleep = TokioSleep::new();
    let config = aws_config::from_env()
        .region(Region::new("us-east-2"))
        .no_credentials()
        .http_client(http_client)
        .sleep_impl(sleep)
        .load()
        .await;

    let client = s3::Client::new(&config);
    let result = client
        .list_objects_v2()
        .bucket("nara-national-archives-catalog")
        .delimiter("/")
        .prefix("authority-records/organization/")
        .max_keys(5)
        .send()
        .await
        .expect("Failed to ListObjects");

    //For ease of modeling the return we just extract the keys from the objects
    let object_names: Vec<String> = result
        .contents
        .expect("No S3 Objects")
        .iter()
        .map(|obj| obj.key().expect("Object has no name").to_string())
        .collect();

    Ok(object_names)
}
