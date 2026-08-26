/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Smithy HTTP client handles resolved to one pool partition.

use super::partition::PartitionId;
use super::registry::PartitionState;
use super::ConnectionPool;
use crate::client::timeout::{self, TimeoutKind};
use crate::sync::Arc;
use aws_smithy_async::rt::sleep::{default_async_sleep, SharedAsyncSleep};
use aws_smithy_runtime_api::client::connector_metadata::ConnectorMetadata;
use aws_smithy_runtime_api::client::http::{
    HttpClient, HttpConnector, HttpConnectorFuture, HttpConnectorSettings, SharedHttpConnector,
};
use aws_smithy_runtime_api::client::orchestrator::{HttpRequest, HttpResponse};
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use std::borrow::Cow;
use std::error::Error;
use std::fmt;
use std::time::Duration;

/// A cheap connection-pool handle fixed to one declared partition.
#[derive(Clone)]
pub struct Client {
    pool: ConnectionPool,
    partition: Arc<PartitionState>,
}

impl Client {
    /// Resolves the pool's anonymous partition.
    pub fn new(pool: &ConnectionPool) -> Result<Self, InvalidPartition> {
        Self::from_partition(pool, PartitionId::ANONYMOUS)
    }

    /// Resolves one explicitly declared partition.
    pub fn from_partition(
        pool: &ConnectionPool,
        id: PartitionId,
    ) -> Result<Self, InvalidPartition> {
        let partition = pool
            .inner
            .registry
            .partition(id)
            .ok_or(InvalidPartition { partition: id })?;
        Ok(Self {
            pool: pool.clone(),
            partition,
        })
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("partition", &self.partition.id())
            .field("pool", &self.pool)
            .finish_non_exhaustive()
    }
}

impl HttpClient for Client {
    fn http_connector(
        &self,
        settings: &HttpConnectorSettings,
        components: &RuntimeComponents,
    ) -> SharedHttpConnector {
        let connect_timeout = settings.connect_timeout();
        let read_timeout = settings.read_timeout();
        let sleep = components.sleep_impl().or_else(default_async_sleep);
        if (connect_timeout.is_some() || read_timeout.is_some()) && sleep.is_none() {
            panic!("an async sleep implementation is required for HTTP connect or read timeouts");
        }

        SharedHttpConnector::new(PoolConnector {
            pool: self.pool.clone(),
            partition: self.partition.clone(),
            connect_timeout,
            read_timeout,
            sleep,
        })
    }

    fn connector_metadata(&self) -> Option<ConnectorMetadata> {
        Some(ConnectorMetadata::new("hyper", Some(Cow::Borrowed("1.x"))))
    }
}

/// Error returned when a client names no partition in the pool.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidPartition {
    partition: PartitionId,
}

impl InvalidPartition {
    /// Returns the unresolved partition identity.
    pub fn partition(&self) -> PartitionId {
        self.partition
    }
}

impl fmt::Display for InvalidPartition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "connection pool does not contain partition {:?}",
            self.partition
        )
    }
}

impl Error for InvalidPartition {}

/// Per-operation timeout facade over one resolved pool partition.
struct PoolConnector {
    pool: ConnectionPool,
    partition: Arc<PartitionState>,
    connect_timeout: Option<Duration>,
    read_timeout: Option<Duration>,
    sleep: Option<SharedAsyncSleep>,
}

impl fmt::Debug for PoolConnector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PoolConnector")
            .field("partition", &self.partition.id())
            .field("connect_timeout", &self.connect_timeout)
            .field("read_timeout", &self.read_timeout)
            .finish_non_exhaustive()
    }
}

impl HttpConnector for PoolConnector {
    fn call(&self, request: HttpRequest) -> HttpConnectorFuture {
        let pool = self.pool.clone();
        let partition = self.partition.clone();
        let connect_timeout = self.connect_timeout;
        let read_timeout = self.read_timeout;
        let sleep = self.sleep.clone();
        HttpConnectorFuture::new(async move {
            let request = request.try_into_http1x().map_err(|error| {
                aws_smithy_runtime_api::client::result::ConnectorError::user(error.into())
            })?;
            let send = pool.send_request(
                partition,
                request,
                connect_timeout.zip(sleep.clone()).map(|(duration, sleep)| {
                    super::handshake::ConnectTimeout::new(duration, sleep)
                }),
            );
            let response = timeout::maybe_timeout_future(
                send,
                read_timeout,
                sleep.as_ref(),
                TimeoutKind::Read,
            )
            .await
            .map_err(super::super::downcast_error)?;
            HttpResponse::try_from(response).map_err(|error| {
                aws_smithy_runtime_api::client::result::ConnectorError::other(error.into(), None)
            })
        })
    }
}
