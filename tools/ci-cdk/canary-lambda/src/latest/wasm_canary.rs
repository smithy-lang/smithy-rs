/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::{mk_canary, CanaryEnv};

use wasmtime::component::{bindgen, Component, Linker};
use wasmtime::{Engine, Store};
use wasmtime_wasi::preview2::{WasiCtxBuilder, WasiView};

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
    wasi_http_ctx: wasmtime_wasi_http::WasiHttpCtx,
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

impl wasmtime_wasi::preview2::bindings::sync_io::io::error::HostError for WasiHostCtx {
    fn to_debug_string(
        &mut self,
        err: wasmtime::component::Resource<
            wasmtime_wasi::preview2::bindings::sync_io::io::error::Error,
        >,
    ) -> wasmtime::Result<String> {
        Ok(format!("{:?}", self.table().get(&err)?))
    }

    fn drop(
        &mut self,
        err: wasmtime::component::Resource<
            wasmtime_wasi::preview2::bindings::sync_io::io::error::Error,
        >,
    ) -> wasmtime::Result<()> {
        self.table_mut().delete(err)?;
        Ok(())
    }
}

impl wasmtime_wasi::preview2::bindings::sync_io::io::error::Host for WasiHostCtx {}

impl wasmtime_wasi_http::types::WasiHttpView for WasiHostCtx {
    fn ctx(&mut self) -> &mut wasmtime_wasi_http::WasiHttpCtx {
        &mut self.wasi_http_ctx
    }

    fn table(&mut self) -> &mut wasmtime::component::ResourceTable {
        &mut self.preview2_table
    }
}

pub fn wasm_canary() -> anyhow::Result<String> {
    let cur_dir = std::env::current_dir().expect("Current dir");
    println!("CURRENT DIR: {cur_dir:#?}");

    // Create a Wasmtime Engine configured to run Components
    let engine = Engine::new(&wasmtime::Config::new().wasm_component_model(true))
        .expect("Failed to create wasm engine");

    // Create our component from the wasm file
    let component = Component::from_file(
        &engine,
        cur_dir.join("aws_sdk_rust_lambda_canary_wasm.wasm"),
    )
    .expect("Failed to load wasm");

    // Create the linker and link in the necessary WASI bindings
    let mut linker: Linker<WasiHostCtx> = Linker::new(&engine);
    wasmtime_wasi::preview2::bindings::sync_io::io::poll::add_to_linker(&mut linker, |cx| cx)
        .expect("Failed to link Poll");
    wasmtime_wasi::preview2::bindings::sync_io::io::error::add_to_linker(&mut linker, |cx| cx)
        .expect("Failed to link Error");
    wasmtime_wasi::preview2::bindings::sync_io::io::streams::add_to_linker(&mut linker, |cx| cx)
        .expect("Failed to link Streams");
    wasmtime_wasi::preview2::bindings::random::random::add_to_linker(&mut linker, |cx| cx)
        .expect("Failed to link Random");
    wasmtime_wasi::preview2::bindings::wasi::clocks::monotonic_clock::add_to_linker(
        &mut linker,
        |cx| cx,
    )
    .expect("Failed to link Clock");
    wasmtime_wasi::preview2::bindings::sync_io::filesystem::types::add_to_linker(
        &mut linker,
        |cx| cx,
    )
    .expect("Failed to link Filesystem Types");
    wasmtime_wasi::preview2::bindings::filesystem::preopens::add_to_linker(&mut linker, |cx| cx)
        .expect("Failed to link Filesystem Preopen");
    wasmtime_wasi::preview2::bindings::wasi::clocks::wall_clock::add_to_linker(&mut linker, |cx| {
        cx
    })
    .expect("Failed to link Wall Clock");
    wasmtime_wasi::preview2::bindings::wasi::cli::environment::add_to_linker(&mut linker, |cx| cx)
        .expect("Failed to link Environment");
    wasmtime_wasi::preview2::bindings::wasi::cli::exit::add_to_linker(&mut linker, |cx| cx)
        .expect("Failed to link Environment");
    wasmtime_wasi::preview2::bindings::wasi::cli::stdin::add_to_linker(&mut linker, |cx| cx)
        .expect("Failed to link Stdin");
    wasmtime_wasi::preview2::bindings::wasi::cli::stdout::add_to_linker(&mut linker, |cx| cx)
        .expect("Failed to link Stdout");
    wasmtime_wasi::preview2::bindings::wasi::cli::stderr::add_to_linker(&mut linker, |cx| cx)
        .expect("Failed to link Stderr");
    // wasmtime_wasi::preview2::command::add_to_linker(&mut linker);
    wasmtime_wasi::preview2::bindings::wasi::cli::terminal_input::add_to_linker(
        &mut linker,
        |cx| cx,
    )
    .expect("Failed to link Terminal Input");
    wasmtime_wasi::preview2::bindings::wasi::cli::terminal_output::add_to_linker(
        &mut linker,
        |cx| cx,
    )
    .expect("Failed to link Terminal Output");
    wasmtime_wasi::preview2::bindings::wasi::cli::terminal_stdin::add_to_linker(
        &mut linker,
        |cx| cx,
    )
    .expect("Failed to link Terminal Stdin");
    wasmtime_wasi::preview2::bindings::wasi::cli::terminal_stdout::add_to_linker(
        &mut linker,
        |cx| cx,
    )
    .expect("Failed to link Terminal Stdout");
    wasmtime_wasi::preview2::bindings::wasi::cli::terminal_stderr::add_to_linker(
        &mut linker,
        |cx| cx,
    )
    .expect("Failed to link Terminal Stderr");
    wasmtime_wasi_http::bindings::http::types::add_to_linker(&mut linker, |cx| cx)
        .expect("Failed to link HTTP Types");
    wasmtime_wasi_http::bindings::http::outgoing_handler::add_to_linker(&mut linker, |cx| cx)
        .expect("Failed to link HTTP Outgoing Handler");

    // Configure and create a `WasiCtx`, which WASI functions need access to
    // through the host state of the store (which in this case is the host state
    // of the store)
    let wasi_ctx = WasiCtxBuilder::new()
        .inherit_stderr()
        .inherit_stdout()
        .build();

    let host_ctx = WasiHostCtx {
        preview2_ctx: wasi_ctx,
        preview2_table: wasmtime_wasi::preview2::ResourceTable::new(),
        preview1_adapter: wasmtime_wasi::preview2::preview1::WasiPreview1Adapter::new(),
        wasi_http_ctx: wasmtime_wasi_http::WasiHttpCtx {},
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
// #[tokio::test]
#[test]
fn test_wasm_canary() {
    let res = wasm_canary();
    println!("{res:#?}");
}
