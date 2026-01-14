/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::{mk_canary, CanaryEnv};

use aws_config::SdkConfig;
use wasmtime::component::{bindgen, Component, Linker};
use wasmtime::{Engine, Store};
use wasmtime_wasi::WasiCtxBuilder;

mk_canary!("wasm", |_sdk_config: &SdkConfig, _env: &CanaryEnv| {
    wasm_canary()
});

//This macro creates bindings to call the wasm functions in Rust
bindgen!({
    world: "canary-world",
    path: "../canary-wasm/wit/component.wit",
    exports: {
        default: async
    },
});

struct WasiHostCtx {
    preview2_ctx: wasmtime_wasi::WasiCtx,
    preview2_table: wasmtime::component::ResourceTable,
    wasi_http_ctx: wasmtime_wasi_http::WasiHttpCtx,
}

impl wasmtime_wasi::WasiView for WasiHostCtx {
    fn ctx(&mut self) -> wasmtime_wasi::WasiCtxView {
        wasmtime_wasi::WasiCtxView {
            ctx: &mut self.preview2_ctx,
            table: &mut self.preview2_table,
        }
    }
}

impl wasmtime_wasi_http::types::WasiHttpView for WasiHostCtx {
    fn ctx(&mut self) -> &mut wasmtime_wasi_http::WasiHttpCtx {
        &mut self.wasi_http_ctx
    }

    fn table(&mut self) -> &mut wasmtime::component::ResourceTable {
        &mut self.preview2_table
    }
}

pub async fn wasm_canary() -> anyhow::Result<()> {
    let wasm_bin_path = std::env::current_dir()
        .expect("Current dir")
        .join("aws_sdk_rust_lambda_canary_wasm.wasm");

    // Create a Wasmtime Engine configured to run Components
    let engine = Engine::new(
        wasmtime::Config::new()
            .wasm_component_model(true)
            .async_support(true),
    )?;

    // Create our component from the wasm file
    let component = Component::from_file(&engine, wasm_bin_path)?;

    // Create the linker and link in the necessary WASI bindings
    let mut linker = Linker::new(&engine);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    wasmtime_wasi_http::add_only_http_to_linker_async(&mut linker)?;

    // Configure and create a `WasiCtx`, which WASI functions need access to
    // through the host state of the store (which in this case is the host state
    // of the store)
    let wasi_ctx = WasiCtxBuilder::new()
        .inherit_stderr()
        .inherit_stdout()
        .build();

    let host_ctx = WasiHostCtx {
        preview2_ctx: wasi_ctx,
        preview2_table: wasmtime_wasi::ResourceTable::new(),
        wasi_http_ctx: wasmtime_wasi_http::WasiHttpCtx::new(),
    };

    let mut store: Store<WasiHostCtx> = Store::new(&engine, host_ctx);

    // Instantiate our module with the bindgen! bindings
    let bindings = CanaryWorld::instantiate_async(&mut store, &component, &linker).await?;

    let canary_interface = bindings.aws_component_canary_interface();
    let api_result = canary_interface
        .call_run_canary(store)
        .await?
        .map_err(anyhow::Error::msg)?;

    assert!(!api_result.is_empty());

    Ok(())
}

#[cfg(test)]
#[tokio::test]
async fn test_wasm_canary() {
    wasm_canary().await.expect("Wasm return")
}
