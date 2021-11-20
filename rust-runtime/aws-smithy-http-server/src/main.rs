use std::sync::{Arc, Mutex};

use aws_smithy_http_server::add_extension::AddExtensionLayer;
use aws_smithy_http_server::extension::Extension;
use aws_smithy_http_server::model::*;
use aws_smithy_http_server::operation_registry::SimpleServiceOperationRegistryBuilder;
use aws_smithy_http_server::routing::Router;

struct State {
    counter: Mutex<i32>,
}

async fn _healthcheck_operation(_input: HealthcheckInput) -> HealthcheckOutput {
    HealthcheckOutput
}

async fn healthcheck_operation_with_state(_input: HealthcheckInput, state: Extension<Arc<State>>) -> HealthcheckOutput {
    let mut counter = state.0.counter.lock().unwrap();
    *counter += 1;
    dbg!(&counter);

    HealthcheckOutput
}

async fn register_service_operation(
    _input: RegisterServiceInput,
) -> Result<RegisterServiceOutput, RegisterServiceError> {
    Ok(RegisterServiceOutput)
}

async fn _register_service_operation_with_state(
    _input: RegisterServiceInput,
    _state: Extension<State>,
) -> Result<RegisterServiceOutput, RegisterServiceError> {
    Ok(RegisterServiceOutput)
}

#[tokio::main]
async fn main() {
    let app: Router = SimpleServiceOperationRegistryBuilder::default()
        // .health_check(_healthcheck_operation)
        .health_check(healthcheck_operation_with_state)
        .register_service(register_service_operation)
        // .register_service(_register_service_operation_with_state)
        .build()
        .unwrap()
        .into();

    let shared_state = std::sync::Arc::new(State { counter: Mutex::new(69) });

    let app = app.layer(AddExtensionLayer::new(shared_state));

    let server = axum::Server::bind(&"0.0.0.0:8080".parse().unwrap()).serve(app.into_make_service());

    if let Err(err) = server.await {
        eprintln!("server error: {}", err);
    }
}
