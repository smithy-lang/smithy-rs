/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Replays the shared scenarios against the schemaSerde=true services and against the branch's
//! schemaSerde=false twins (HTTP 1.x), and compares both with `baselines/mainline.json`, the
//! responses recorded from unmodified mainline codegen by `protocol-compat-mainline`
//! (`make mainline-baseline`).
//!
//! Every difference is pinned in `baselines/*_divergences.json`; an unlisted difference, or a pinned one
//! that changes or disappears, fails the test. `UPDATE_DIVERGENCES=1` rewrites the pins.
pub use protocol_compat::kit::{from_bytes, Body};
mod support;

use std::collections::BTreeMap;

use protocol_compat::{
    awsjson, cbor, document, legacy_awsjson, legacy_cbor, legacy_document, legacy_rest, rest,
};
use support::scenarios::*;
use support::*;

type Baseline = BTreeMap<String, String>;

fn mainline() -> Baseline {
    serde_json::from_str(include_str!("../baselines/mainline.json"))
        .expect("baselines/mainline.json")
}

async fn replay<S, B>(
    service: impl Fn() -> S,
    protocol: &str,
    scenarios: &[(&'static str, Build)],
) -> Baseline
where
    S: tower::Service<http::Request<Body>, Response = http::Response<B>>,
    S::Error: std::fmt::Debug,
    B: http_body::Body,
    B::Error: std::fmt::Debug,
{
    let mut out = Baseline::new();
    for (name, build) in scenarios {
        out.insert(
            format!("{protocol}/{name}"),
            snapshot(&send(service(), build()).await),
        );
    }
    out
}

macro_rules! all_replays {
    ($rest:expr, $awsjson:expr, $cbor:expr, $document:expr) => {{
        let mut all = Baseline::new();
        all.extend(replay($rest, "restJson1", &rest_scenarios()).await);
        all.extend(replay($awsjson, "awsJson1_0", &aws_json_scenarios()).await);
        all.extend(replay($cbor, "rpcv2Cbor", &cbor_scenarios()).await);
        all.extend(replay($document, "restJson1-document", &document_scenarios()).await);
        all
    }};
}

fn differences(baseline: &Baseline, actual: &Baseline) -> Vec<(String, String, String)> {
    assert_eq!(
        baseline.keys().collect::<Vec<_>>(),
        actual.keys().collect::<Vec<_>>(),
        "scenario sets differ; re-record the baseline"
    );
    baseline
        .iter()
        .filter(|(key, expected)| actual[*key] != **expected)
        .map(|(key, expected)| (key.clone(), expected.clone(), actual[key].clone()))
        .collect()
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Debug)]
struct Divergence {
    mainline: String,
    actual: String,
    /// Why the difference exists, or `TODO(protocol-compat divergence triage)` until someone has triaged it.
    note: String,
}

/// Compares `found` with the pinned file `baselines/<file>`. Set `UPDATE_DIVERGENCES=1` to rewrite
/// the file from the current run (notes of unchanged entries are kept).
fn pin(file: &str, found: Vec<(String, String, String)>) {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/baselines/").to_owned() + file;
    let mut pinned: BTreeMap<String, Divergence> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    if std::env::var_os("UPDATE_DIVERGENCES").is_some() {
        let updated: BTreeMap<String, Divergence> = found
            .into_iter()
            .map(|(key, mainline, actual)| {
                let note = pinned
                    .get(&key)
                    .map(|d| d.note.clone())
                    .unwrap_or_else(|| "TODO(protocol-compat divergence triage)".to_owned());
                (
                    key,
                    Divergence {
                        mainline,
                        actual,
                        note,
                    },
                )
            })
            .collect();
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&updated).unwrap() + "\n",
        )
        .unwrap();
        return;
    }
    let mut problems = Vec::new();
    for (key, mainline, actual) in &found {
        match pinned.remove(key) {
            None => problems.push(format!("NEW DIVERGENCE {key}\n  mainline: {mainline}\n  actual:   {actual}")),
            Some(d) if d.mainline != *mainline || d.actual != *actual => {
                problems.push(format!("CHANGED DIVERGENCE {key}\n  mainline: {mainline}\n  actual:   {actual}\n  pinned:   {}", d.actual))
            }
            Some(_) => {}
        }
    }
    problems.extend(
        pinned
            .keys()
            .map(|key| format!("RESOLVED, remove from {file}: {key}")),
    );
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

#[tokio::test]
async fn schema_matches_mainline_except_known_divergences() {
    let baseline = mainline();
    assert!(
        !baseline.is_empty(),
        "baselines/mainline.json is empty; run `make mainline-baseline`"
    );
    let actual = all_replays!(
        || rest::service(),
        || awsjson::service(),
        || cbor::service(),
        || document::service()
    );
    pin("schema_divergences.json", differences(&baseline, &actual));
}

/// The branch also edits the shared (schemaSerde=false) generator; its output must stay mainline's.
#[tokio::test]
async fn branch_legacy_path_matches_mainline_except_known_divergences() {
    let baseline = mainline();
    assert!(
        !baseline.is_empty(),
        "baselines/mainline.json is empty; run `make mainline-baseline`"
    );
    let actual = all_replays!(
        || legacy_rest::service(),
        || legacy_awsjson::service(),
        || legacy_cbor::service(),
        || legacy_document::service()
    );
    pin(
        "branch_legacy_divergences.json",
        differences(&baseline, &actual),
    );
}
