/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![allow(missing_docs)]

use aws_smithy_http_server_metrics::smithy_metrics;
use metrique::unit_of_work::metrics;
#[smithy_metrics(rename(service_name = "program"))]
#[metrics]
#[derive(Default)]
pub struct PokemonMetrics {
    #[metrics(flatten)]
    pub request_metrics: PokemonRequestMetrics,
    #[metrics(flatten)]
    pub response_metrics: PokemonResponseMetrics,
    #[metrics(flatten)]
    #[smithy_metrics(operation)]
    pub operation_metrics: PokemonOperationMetrics,
}

#[metrics]
#[derive(Default, Clone)]
pub struct PokemonOperationMetrics {
    #[metrics(flatten)]
    pub get_pokemon_species_metrics: GetPokemonSpeciesMetrics,
    #[metrics(flatten)]
    pub get_storage_metrics: GetStorageMetrics,
    #[metrics(flatten)]
    pub get_server_statistics_metrics: GetServerStatisticsMetrics,
    #[metrics(flatten)]
    pub capture_pokemon_metrics: CapturePokemonMetrics,
    #[metrics(flatten)]
    pub check_health_metrics: CheckHealthMetrics,
    #[metrics(flatten)]
    pub stream_pokemon_radio_metrics: StreamPokemonRadioMetrics,
}

#[metrics]
#[derive(Default, Clone)]
pub struct GetPokemonSpeciesMetrics {
    pub requested_pokemon_name: Option<String>,
    pub found: Option<bool>,
}

#[metrics]
#[derive(Default, Clone)]
pub struct GetStorageMetrics {
    pub user: Option<String>,
    pub authenticated: Option<bool>,
}

#[metrics]
#[derive(Default, Clone)]
pub struct GetServerStatisticsMetrics {
    pub total_calls: Option<String>,
}

#[metrics]
#[derive(Default, Clone)]
pub struct CapturePokemonMetrics {
    pub requested_region: Option<String>,
    pub supported_region: Option<bool>,
}

#[metrics]
#[derive(Default, Clone)]
pub struct CheckHealthMetrics {
    pub health_check_count: Option<usize>,
}

#[metrics]
#[derive(Default, Clone)]
pub struct StreamPokemonRadioMetrics {
    pub stream_url: Option<String>,
}

#[metrics]
#[derive(Default, Clone)]
pub struct PokemonRequestMetrics {
    pub test_request_metric: Option<String>,
}

#[metrics]
#[derive(Default, Clone)]
pub struct PokemonResponseMetrics {
    pub test_response_metric: Option<String>,
}
