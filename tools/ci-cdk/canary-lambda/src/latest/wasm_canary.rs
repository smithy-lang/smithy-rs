/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::{mk_canary, CanaryEnv};

use wasmtime::component::{bindgen, Component, Linker};
use wasmtime::{Engine, Store};
use wasmtime_wasi::preview2::WasiCtxBuilder;
use wasmtime_wasi::WasiCtx;

// mk_canary!("wasm", |sdk_config: &SdkConfig, env: &CanaryEnv| {
//     wasm_canary()
// });

//This macro creates bindings to call the wasm functions in Rust
//TODO: this should point to a wit file in canary-wasm
bindgen!({
    inline: "
    package aws:component;

    interface canary-interface {
        run-canary: func() -> result<string, string>;
    }

    world canary-world {
        export canary-interface;
    }
",
with: {
    "wasi:io/error": wasmtime_wasi::preview2::bindings::io::error,
    "wasi:io/streams": wasmtime_wasi::preview2::bindings::io::streams,
    "wasi:io/poll": wasmtime_wasi::preview2::bindings::io::poll,
}
});

struct WasiHostCtx {
    preview2_ctx: wasmtime_wasi::preview2::WasiCtx,
    preview2_table: wasmtime::component::ResourceTable,
    preview1_adapter: wasmtime_wasi::preview2::preview1::WasiPreview1Adapter,
}

impl wasmtime_wasi::preview2::WasiView for WasiHostCtx {
    fn table(&self) -> &wasmtime::component::ResourceTable {
        &self.preview2_table
    }

    fn ctx(&self) -> &wasmtime_wasi::preview2::WasiCtx {
        &self.preview2_ctx
    }

    fn table_mut(&mut self) -> &mut wasmtime::component::ResourceTable {
        &mut self.preview2_table
    }

    fn ctx_mut(&mut self) -> &mut wasmtime_wasi::preview2::WasiCtx {
        &mut self.preview2_ctx
    }
}

impl wasmtime_wasi::preview2::preview1::WasiPreview1View for WasiHostCtx {
    fn adapter(&self) -> &wasmtime_wasi::preview2::preview1::WasiPreview1Adapter {
        &self.preview1_adapter
    }

    fn adapter_mut(&mut self) -> &mut wasmtime_wasi::preview2::preview1::WasiPreview1Adapter {
        &mut self.preview1_adapter
    }
}

pub async fn wasm_canary() -> anyhow::Result<String> {
    let cur_dir = std::env::current_dir().expect("Current dir");
    println!("CURRENT DIR: {cur_dir:#?}");

    // Create a Wasmtime Engine configured to run Components
    let mut engine_config = wasmtime::Config::new();
    engine_config.wasm_component_model(true);
    let engine = Engine::new(&engine_config).expect("Failed to create wasm engine");

    // Create our component from the wasm file
    let component = Component::from_file(
        &engine,
        cur_dir.join("aws_sdk_rust_lambda_canary_wasm.wasm"),
    )
    .expect("Failed to load wasm");
    let mut linker: Linker<WasiHostCtx> = Linker::new(&engine);
    // wasmtime_wasi::preview2::preview1::add_to_linker_async(&mut linker);
    // wasmtime_wasi::preview2::bindings::io::poll::add_to_linker(&mut linker, |cx| cx);

    // Configure and create a `WasiCtx`, which WASI functions need access to
    // through the host state of the store (which in this case is the host state
    // of the store)
    let wasi_ctx = WasiCtxBuilder::new()
        .inherit_stdio()
        .inherit_stderr()
        .inherit_stdout()
        .inherit_network()
        .build();

    let host_ctx = WasiHostCtx {
        preview2_ctx: wasi_ctx,
        preview2_table: wasmtime_wasi::preview2::ResourceTable::new(),
        preview1_adapter: wasmtime_wasi::preview2::preview1::WasiPreview1Adapter::new(),
    };

    let mut store: Store<WasiHostCtx> = Store::new(&engine, host_ctx);

    // Instantiate our module with the bindgen! bindings
    let (bindings, _) = CanaryWorld::instantiate(&mut store, &component, &linker)?;

    println!("CALLING WASM FUNC");
    let canary_interface = bindings.aws_component_canary_interface();
    let api_result = canary_interface
        .call_run_canary(store)?
        .map_err(|err| anyhow::Error::msg(err))?;
    println!("{api_result}");
    Ok(api_result)
}

// #[ignore]
#[cfg(test)]
#[tokio::test]
async fn test_wasm_canary() {
    let res = wasm_canary().await;
    println!("{res:#?}");
}
