/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use command_group::{CommandGroup, GroupChild};
use pokemon_service_client::{Builder, Client, Config};
use std::{process::Command, thread, time::Duration};

pub(crate) struct PokemonService {
    // We need to ensure all processes forked by the Python interpreter
    // are on the same process group, otherwise only the main process
    // will be killed during drop, leaving the test worker alive.
    child_process: GroupChild,
}

impl PokemonService {
    #[allow(dead_code)]
    pub(crate) fn run() -> Self {
        let process = Command::new("python3")
            .arg("../pokemon_service.py")
            .group_spawn()
            .expect("failed to spawn the Pokémon Service program");
        // The Python interpreter takes a little to startup.
        thread::sleep(Duration::from_secs(2));
        Self {
            child_process: process,
        }
    }
}

impl Drop for PokemonService {
    fn drop(&mut self) {
        self.child_process
            .kill()
            .expect("failed to kill Pokémon Service program");
        self.child_process.wait().ok();
    }
}

#[allow(dead_code)]
pub fn client() -> Client<
    aws_smithy_client::erase::DynConnector,
    aws_smithy_client::erase::DynMiddleware<aws_smithy_client::erase::DynConnector>,
> {
    let raw_client = Builder::new()
        .rustls()
        .middleware_fn(|mut req| {
            let http_req = req.http_mut();
            let uri = format!("http://localhost:13734{}", http_req.uri().path());
            *http_req.uri_mut() = uri.parse().unwrap();
            req
        })
        .build_dyn();
    let config = Config::builder().build();
    Client::with_config(raw_client, config)
}
