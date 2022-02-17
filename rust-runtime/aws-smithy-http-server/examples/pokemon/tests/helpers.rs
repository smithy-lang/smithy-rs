use amzn_pokemon_service_client::{Builder, Client, Config};

pub(crate) struct PokemonService {
    child_process: std::process::Child,
}

impl PokemonService {
    pub(crate) fn run() -> Self {
        use assert_cmd::prelude::*;
        use std::process::Command;
        let process = Command::cargo_bin("amzn-pokemon-service").unwrap().spawn().unwrap();

        Self { child_process: process }
    }
}

impl Drop for PokemonService {
    fn drop(&mut self) {
        self.child_process
            .kill()
            .expect("failed to kill Pokémon Service program")
    }
}

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
