use aws_smithy_http_server_metrics::smithy_metrics;
use metrique::unit_of_work::metrics;

#[smithy_metrics(server_crate = pokemon_service_server_sdk)]
#[metrics]
#[derive(Default)]
pub(crate) struct PokemonMetrics {
    #[metrics(flatten)]
    pub(crate) request_metrics: PokemonRequestMetrics,
    #[metrics(flatten)]
    pub(crate) response_metrics: PokemonResponseMetrics,
}

#[metrics]
#[derive(Default)]
pub(crate) struct PokemonRequestMetrics {
    pub(crate) test_request_metric: Option<String>,
}

#[metrics]
#[derive(Default)]
pub(crate) struct PokemonResponseMetrics {
    pub(crate) test_response_metric: Option<String>,
}
