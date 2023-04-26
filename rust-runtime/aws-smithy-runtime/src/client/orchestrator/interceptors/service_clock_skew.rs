/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::client::orchestrator::{HttpRequest, HttpResponse};
use aws_smithy_runtime_api::client::interceptors::{BoxError, Interceptor, InterceptorContext};
use aws_smithy_runtime_api::config_bag::ConfigBag;
use aws_smithy_types::date_time::Format;
use aws_smithy_types::DateTime;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ServiceClockSkew {
    inner: Duration,
}

impl ServiceClockSkew {
    fn new(inner: Duration) -> Self {
        Self { inner }
    }

    pub fn skew(&self) -> Duration {
        self.inner
    }
}

impl From<ServiceClockSkew> for Duration {
    fn from(skew: ServiceClockSkew) -> Duration {
        skew.inner
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub struct ServiceClockSkewInterceptor {}

impl ServiceClockSkewInterceptor {
    pub fn new() -> Self {
        Self {}
    }
}

fn calculate_skew(time_sent: DateTime, time_received: DateTime) -> Duration {
    let skew = (time_sent.as_secs_f64() - time_received.as_secs_f64()).max(0.0);
    Duration::from_secs_f64(skew)
}

fn extract_time_sent_from_response(
    ctx: &mut InterceptorContext<HttpRequest, HttpResponse>,
) -> Result<DateTime, BoxError> {
    let res = ctx.request()?;
    let date_header = res
        .headers()
        .get("date")
        .ok_or_else(|| "Response from server is missing expected 'date' header")?
        .to_str()
        .expect("date header is always valid UTF-8");
    DateTime::from_str(date_header, Format::HttpDate).map_err(Into::into)
}

impl Interceptor<HttpRequest, HttpResponse> for ServiceClockSkewInterceptor {
    fn modify_before_deserialization(
        &self,
        ctx: &mut InterceptorContext<HttpRequest, HttpResponse>,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let time_received = DateTime::from(SystemTime::now());
        let time_sent = match extract_time_sent_from_response(ctx) {
            Ok(time_sent) => time_sent,
            Err(e) => {
                // We don't want to fail a request on account of this, but it's very strange for
                // this to fail, so we emit a log.
                tracing::warn!(
                    "failed to calculate clock skew of service from response: {}",
                    e
                );
                return Ok(());
            }
        };
        let skew = ServiceClockSkew::new(calculate_skew(time_sent, time_received));
        cfg.put(skew);
        Ok(())
    }
}
