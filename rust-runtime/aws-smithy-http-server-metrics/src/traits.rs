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
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
}
impl<T, E, S> InitMetrics<E, S> for T
where
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
    T: Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static,
{
}

pub trait SetRequestMetrics<E, S>:
    Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static
{
}
impl<T, E, S> SetRequestMetrics<E, S> for T where
    T: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static
{
}

pub trait SetResponseMetrics<E, S>:
    Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static
{
}
impl<T, E, S> SetResponseMetrics<E, S> for T where
    T: Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static
{
}
