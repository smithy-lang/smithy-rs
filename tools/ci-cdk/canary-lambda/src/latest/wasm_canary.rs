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

    println!("WASM BIN PATH: {wasm_bin_path:#?}");

    // Create a Wasmtime Engine configured to run Components
    let engine = Engine::new(
        wasmtime::Config::new()
            .wasm_component_model(true)
            .async_support(true),
    )?;

    // Create our component from the wasm file
    let component = Component::from_file(&engine, wasm_bin_path)?;

    // Create the linker and link in the necessary WASI bindings
    // let mut linker: Linker<WasiHostCtx> = Linker::new(&engine);
    // link_all_the_things(&mut linker);

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

/// This function adds all of the WASI bindings to the linker
// fn link_all_the_things(linker: &mut Linker<WasiHostCtx>) {
//     //IO
//     wasmtime_wasi::p2::bindings::io::poll::add_to_linker(linker, |cx| cx)
//         .expect("Failed to link Poll");
//     wasmtime_wasi::p2::bindings::io::error::add_to_linker(linker, |cx| cx)
//         .expect("Failed to link Error");
//     wasmtime_wasi::p2::bindings::io::streams::add_to_linker(linker, |cx| cx)
//         .expect("Failed to link Streams");

//     //Random
//     wasmtime_wasi::p2::bindings::random::random::add_to_linker(linker, |cx| cx)
//         .expect("Failed to link Random");

//     //Clocks
//     wasmtime_wasi::p2::bindings::clocks::monotonic_clock::add_to_linker(linker, |cx| cx)
//         .expect("Failed to link Clock");
//     wasmtime_wasi::p2::bindings::clocks::wall_clock::add_to_linker(linker, |cx| cx)
//         .expect("Failed to link Wall Clock");

//     //Filesystem
//     wasmtime_wasi::p2::bindings::filesystem::types::add_to_linker(linker, |cx| cx)
//         .expect("Failed to link Filesystem Types");
//     wasmtime_wasi::p2::bindings::filesystem::preopens::add_to_linker(linker, |cx| cx)
//         .expect("Failed to link Filesystem Preopen");

//     //CLI
//     wasmtime_wasi::p2::bindings::cli::environment::add_to_linker(linker, |cx| cx)
//         .expect("Failed to link Environment");
//     wasmtime_wasi::p2::bindings::cli::exit::add_to_linker(linker, |cx| cx)
//         .expect("Failed to link Environment");
//     wasmtime_wasi::p2::bindings::cli::stdin::add_to_linker(linker, |cx| cx)
//         .expect("Failed to link Stdin");
//     wasmtime_wasi::p2::bindings::cli::stdout::add_to_linker(linker, |cx| cx)
//         .expect("Failed to link Stdout");
//     wasmtime_wasi::p2::bindings::cli::stderr::add_to_linker(linker, |cx| cx)
//         .expect("Failed to link Stderr");

//     // CLI Terminal
//     wasmtime_wasi::p2::bindings::cli::terminal_input::add_to_linker(linker, |cx| cx)
//         .expect("Failed to link Terminal Input");
//     wasmtime_wasi::p2::bindings::cli::terminal_output::add_to_linker(linker, |cx| cx)
//         .expect("Failed to link Terminal Output");
//     wasmtime_wasi::p2::bindings::cli::terminal_stdin::add_to_linker(linker, |cx| cx)
//         .expect("Failed to link Terminal Stdin");
//     wasmtime_wasi::p2::bindings::cli::terminal_stdout::add_to_linker(linker, |cx| cx)
//         .expect("Failed to link Terminal Stdout");
//     wasmtime_wasi::p2::bindings::cli::terminal_stderr::add_to_linker(linker, |cx| cx)
//         .expect("Failed to link Terminal Stderr");

//     //HTTP
//     wasmtime_wasi_http::bindings::http::types::add_to_linker(linker, |cx| cx)
//         .expect("Failed to link HTTP Types");
//     wasmtime_wasi_http::bindings::http::outgoing_handler::add_to_linker(linker, |cx| cx)
//         .expect("Failed to link HTTP Outgoing Handler");
// }

// #[ignore]
#[cfg(test)]
#[tokio::test]
async fn test_wasm_canary() {
    wasm_canary().await.expect("Wasm return")
}
