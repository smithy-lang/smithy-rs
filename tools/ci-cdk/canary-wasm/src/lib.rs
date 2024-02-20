/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::Region;
use aws_sdk_s3 as s3;
use aws_smithy_async::rt::sleep::{AsyncSleep, Sleep};
use aws_smithy_wasm::wasi::WasiHttpClientBuilder;

// Needed for WASI-compliant environment as it expects specific functions
// to be exported such as `cabi_realloc`, `_start`, etc.
wit_bindgen::generate!({
    inline: "
         package aws:component;
 
         interface canary-interface {
             run-canary: func() -> result<string, string>;
         }
 
         world canary-world {
             export canary-interface;
         }
     ",
    exports: {
        "aws:component/canary-interface": Component
    }
});

struct Component;

impl exports::aws::component::canary_interface::Guest for Component {
    fn run_canary() -> Result<String, String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("Failed to generate runtime");
        let res = rt.block_on(run_canary())?;
        Ok(res)
    }
}

async fn run_canary() -> Result<String, String> {
    let http_client = WasiHttpClientBuilder::new().build();
    let config = aws_config::from_env()
        .region(Region::new("us-east-2"))
        .no_credentials()
        .http_client(http_client)
        .sleep_impl(WasmSleep)
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

    println!("WASM CANARY RESULT: {result:#?}");
    Ok(format!("{result:?}"))
}

#[derive(Debug, Clone)]
struct WasmSleep;
impl AsyncSleep for WasmSleep {
    fn sleep(&self, duration: std::time::Duration) -> Sleep {
        Sleep::new(Box::pin(async move {
            tokio::time::sleep(duration).await;
        }))
    }
}
