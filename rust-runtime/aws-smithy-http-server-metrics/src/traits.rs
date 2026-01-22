/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use http::Request;
use http::Response;
use metrique::AppendAndCloseOnDrop;
use metrique::RootEntry;
use metrique_core::CloseEntry;
use metrique_writer::EntrySink;

use crate::types::ReqBody;
use crate::types::ResBody;

pub trait MetriqueCloseEntry: CloseEntry + Send + Sync + 'static {}
impl<T> MetriqueCloseEntry for T where T: CloseEntry + Send + Sync + 'static {}

pub trait MetriqueEntrySink<E>: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static
where
    E: CloseEntry + Send + Sync + 'static,
{
}
impl<T, E> MetriqueEntrySink<E> for T
where
    E: CloseEntry + Send + Sync + 'static,
    T: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
}

pub trait InitMetrics<E, S>:
    Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static
where
    E: MetriqueCloseEntry,
    S: MetriqueEntrySink<E>,
{
}
impl<T, E, S> InitMetrics<E, S> for T
where
    E: MetriqueCloseEntry,
    S: MetriqueEntrySink<E>,
    T: Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static,
{
}

pub trait RequestMetrics<E>:
    Fn(&mut Request<ReqBody>, &mut E) + Clone + Send + Sync + 'static
where
    E: MetriqueCloseEntry,
{
}
impl<T, E> RequestMetrics<E> for T
where
    E: MetriqueCloseEntry,
    T: Fn(&mut Request<ReqBody>, &mut E) + Clone + Send + Sync + 'static,
{
}

pub trait ResponseMetrics<E>:
    Fn(&mut Response<ResBody>, &mut E) + Clone + Send + Sync + 'static
where
    E: MetriqueCloseEntry,
{
}
impl<T, E> ResponseMetrics<E> for T
where
    E: MetriqueCloseEntry,
    T: Fn(&mut Response<ResBody>, &mut E) + Clone + Send + Sync + 'static,
{
}
