/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

mod default_config;
mod list_objects;

#[no_mangle]
pub extern "C" fn run() {
    std::panic::set_hook(Box::new(move |panic_info| {
        println!("Internal unhandled panic:\n{:?}!", panic_info);
        std::process::exit(1);
    }));
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    rt.block_on(async move {
        let result = crate::list_objects::s3_list_objects().await;
        println!("result: {:?}", result);
    });
}
