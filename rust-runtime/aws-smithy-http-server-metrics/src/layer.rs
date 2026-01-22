/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![allow(missing_docs)]

use std::marker::PhantomData;

use http::Request;
use http::Response;
use metrique::AppendAndCloseOnDrop;
use metrique::DefaultSink;
use thiserror::Error;
use tower::Layer;

use crate::default::DefaultMetrics;
use crate::default::DefaultRequestMetricsConfig;
use crate::default::DefaultResponseMetricsConfig;
use crate::layer::builder::DefaultMetricsBuildExt;
use crate::layer::builder::MetricsLayerBuilder;
use crate::layer::builder::NeedsInitialization;
use crate::service::MetricsLayerService;
use crate::traits::InitMetrics;
use crate::traits::MetriqueCloseEntry;
use crate::traits::MetriqueEntrySink;
use crate::traits::SetRequestMetrics;
use crate::traits::SetResponseMetrics;
use crate::types::DefaultInit;
use crate::types::DefaultRq;
use crate::types::DefaultRs;
use crate::types::ReqBody;
use crate::types::ResBody;

pub mod builder;

#[derive(Error, Debug)]
pub enum DefaultMetricsLayerError {
    #[error("No sink attached to [`metrique::ServiceMetrics`]")]
    NoSinkAttached,
}

#[derive(Debug)]
pub struct MetricsLayer<
    E = DefaultMetrics,
    S = DefaultSink,
    I = DefaultInit<E, S>,
    Rq = DefaultRq<E, S>,
    Rs = DefaultRs<E, S>,
> where
    E: MetriqueCloseEntry,
    S: MetriqueEntrySink<E>,
    I: InitMetrics<E, S>,
    Rq: SetRequestMetrics<E, S>,
    Rs: SetResponseMetrics<E, S>,
{
    pub(crate) init_metrics: I,
    pub(crate) set_request_metrics: Option<Rq>,
    pub(crate) set_response_metrics: Option<Rs>,
    pub(crate) default_req_metrics_extension_fn:
        fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>, DefaultRequestMetricsConfig),
    pub(crate) default_res_metrics_extension_fn:
        fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>, DefaultResponseMetricsConfig),
    pub(crate) default_req_metrics_config: DefaultRequestMetricsConfig,
    pub(crate) default_res_metrics_config: DefaultResponseMetricsConfig,
}

impl<E, S, I, Rq, Rs> MetricsLayer<E, S, I, Rq, Rs>
where
    E: MetriqueCloseEntry,
    S: MetriqueEntrySink<E>,
    I: InitMetrics<E, S>,
    Rq: SetRequestMetrics<E, S>,
    Rs: SetResponseMetrics<E, S>,
{
    #[doc(hidden)]
    pub fn __macro_new(
        init_metrics: I,
        set_request_metrics: Option<Rq>,
        set_response_metrics: Option<Rs>,
        default_req_metrics_extension_fn: fn(
            &mut Request<ReqBody>,
            &mut AppendAndCloseOnDrop<E, S>,
            DefaultRequestMetricsConfig,
        ),
        default_res_metrics_extension_fn: fn(
            &mut Response<ResBody>,
            &mut AppendAndCloseOnDrop<E, S>,
            DefaultResponseMetricsConfig,
        ),
        default_req_metrics_config: DefaultRequestMetricsConfig,
        default_res_metrics_config: DefaultResponseMetricsConfig,
    ) -> Self {
        Self {
            init_metrics,
            set_request_metrics,
            set_response_metrics,
            default_req_metrics_extension_fn,
            default_res_metrics_extension_fn,
            default_req_metrics_config,
            default_res_metrics_config,
        }
    }
}

impl<S> MetricsLayer<DefaultMetrics, S>
where
    S: MetriqueEntrySink<DefaultMetrics> + Clone,
{
    pub fn new_with_sink(
        sink: S,
    ) -> MetricsLayer<DefaultMetrics, S, impl InitMetrics<DefaultMetrics, S>> {
        Self::builder()
            .init_metrics(move || DefaultMetrics::default().append_on_drop(sink.clone()))
            .build()
    }
}

impl<E, S> MetricsLayer<E, S>
where
    E: MetriqueCloseEntry,
    S: MetriqueEntrySink<E>,
{
    pub fn builder() -> MetricsLayerBuilder<NeedsInitialization, E, S> {
        MetricsLayerBuilder {
            init_metrics: None,
            set_request_metrics: None,
            set_response_metrics: None,
            default_req_metrics_config: DefaultRequestMetricsConfig::default(),
            default_res_metrics_config: DefaultResponseMetricsConfig::default(),
            _state: PhantomData,
            _close_entry: PhantomData,
            _entry_sink: PhantomData,
        }
    }
}

impl<Ser, E, S, I, Rq, Rs> Layer<Ser> for MetricsLayer<E, S, I, Rq, Rs>
where
    Ser: Clone,
    E: MetriqueCloseEntry,
    S: MetriqueEntrySink<E>,
    I: InitMetrics<E, S>,
    Rq: SetRequestMetrics<E, S>,
    Rs: SetResponseMetrics<E, S>,
{
    type Service = MetricsLayerService<Ser, E, S, I, Rq, Rs>;

    fn layer(&self, inner: Ser) -> Self::Service {
        MetricsLayerService {
            inner,
            init_metrics: self.init_metrics.clone(),
            set_request_metrics: self.set_request_metrics.clone(),
            set_response_metrics: self.set_response_metrics.clone(),
            default_req_metrics_extension_fn: self.default_req_metrics_extension_fn,
            default_res_metrics_extension_fn: self.default_res_metrics_extension_fn,
            default_req_metrics_config: self.default_req_metrics_config.clone(),
            default_res_metrics_config: self.default_res_metrics_config.clone(),
        }
    }
}
