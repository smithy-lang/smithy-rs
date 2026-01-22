use aws_smithy_http_server_metrics::smithy_metrics;
use metrique::unit_of_work::metrics;

#[smithy_metrics]
#[metrics]
#[derive(Default)]
pub struct PokemonMetrics {
    #[metrics(flatten)]
    pub request_metrics: PokemonRequestMetrics,
    #[metrics(flatten)]
    pub response_metrics: PokemonResponseMetrics,
    #[metrics(flatten)]
    #[smithy_metrics(operation)]
    pub operation_metrics: OperationMetrics,
}

#[metrics]
#[derive(Default)]
pub struct OperationMetrics {
    pub get_pokemon_species_metrics: Option<String>,
}

#[metrics]
#[derive(Default)]
pub struct PokemonRequestMetrics {
    pub test_request_metric: Option<String>,
}

#[metrics]
#[derive(Default)]
pub struct PokemonResponseMetrics {
    pub test_response_metric: Option<String>,
}
