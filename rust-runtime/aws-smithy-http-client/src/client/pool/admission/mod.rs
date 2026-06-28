/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Admission control for new transport connections.
//!
//! Stacked tower Services that gate a new connection and record it. Each owns
//! one concern; they nest outermost-to-innermost:
//!
//! - [`ConnectionLimit`] — acquires per-host then global semaphore permits,
//!   enforcing the pool's connection cap.
//! - [`ConnectAccounting`] — creates an [`EstablishingGuard`] before the
//!   connector runs, so the `establishing` counter reflects the commitment
//!   point.
//! - [`ConnectRateLimit`] — paces new-connection establishment to measured
//!   establishment capacity.
//! - [`ConnectTimeout`] — bounds the TCP + TLS connector call with the
//!   per-request connect timeout.
//!
//! Stack order is cap → accounting → rate → timeout → connector. Each layer
//! attaches its contribution to [`EstablishedConnection`] on the unwind: the
//! connector produces the IO, accounting the [`EstablishingGuard`], and the
//! cap layer the [`ConnectionPermit`]. Because the cap, accounting, and rate
//! layers wrap the timeout layer, time spent acquiring a permit or waiting to
//! be paced is not charged against the connect-timeout budget.
//!
//! [`EstablishedConnection`]: super::connection::EstablishedConnection
//! [`EstablishingGuard`]: super::stats::EstablishingGuard
//! [`ConnectionPermit`]: super::connection::ConnectionPermit

mod accounting;
mod limit;
mod rate;
mod timeout;

pub(crate) use accounting::ConnectAccounting;
pub(crate) use limit::ConnectionLimit;
pub use rate::ConnectRateConfig;
pub(crate) use rate::{ConnectRateController, ConnectRateLimit};
pub(crate) use timeout::ConnectTimeout;
