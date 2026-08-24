/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::sync::{Arc, Mutex};

use clap::Parser;
use pokemon_service_client::{
    config::{
        interceptors::BeforeTransmitInterceptorContextRef, ConfigBag, Intercept, RuntimeComponents,
    },
    error::BoxError,
    Client, Config,
};
use pokemon_service_client_usage::POKEMON_SERVICE_URL;

const EXPECTED_ACCEPT: &str = "application/json";

#[derive(Debug, Parser)]
#[command(about = "Reproduce protocol-swapping behavior for the Accept header")]
struct Args {
    /// Pokemon service endpoint.
    #[arg(long, default_value = POKEMON_SERVICE_URL)]
    endpoint: String,
}

#[derive(Debug, Clone, Default)]
struct AcceptHeaderCapture {
    value: Arc<Mutex<Option<String>>>,
}

impl Intercept for AcceptHeaderCapture {
    fn name(&self) -> &'static str {
        "AcceptHeaderCapture"
    }

    fn read_after_serialization(
        &self,
        context: &BeforeTransmitInterceptorContextRef<'_>,
        _runtime_components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let value = context
            .request()
            .headers()
            .get("accept")
            .map(ToOwned::to_owned);
        *self.value.lock().expect("accept capture lock poisoned") = value;
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let capture = AcceptHeaderCapture::default();
    let captured_value = capture.value.clone();
    let config = Config::builder()
        .endpoint_url(args.endpoint)
        .protocol(
            aws_smithy_json::protocol::aws_rest_json_1::AwsRestJsonProtocol::new()
                .with_default_namespace("com.aws.example"),
        )
        .interceptor(capture)
        .build();
    let client = Client::from_conf(config);

    let result = client.get_server_statistics().send().await;
    let actual = captured_value
        .lock()
        .expect("accept capture lock poisoned")
        .clone();

    println!("expected Accept={EXPECTED_ACCEPT:?}");
    println!("actual Accept={actual:?}");
    println!("result={result:#?}");

    if actual.as_deref() != Some(EXPECTED_ACCEPT) {
        return Err(format!(
            "runtime RestJson1 request used the modeled protocol's Accept header: {actual:?}"
        )
        .into());
    }

    result?;
    Ok(())
}
