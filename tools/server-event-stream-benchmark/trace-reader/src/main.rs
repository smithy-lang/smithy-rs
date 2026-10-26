// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Decode one case's ordered trace segments into one JSON line per closed request.
use dial9_trace_format::decoder::Decoder;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{self, BufWriter, Write};

#[derive(Default)]
struct Requests {
    pending: HashMap<u64, Value>,
    closed: u64,
}

impl Requests {
    fn event(&mut self, name: &str, value: Value) -> Result<Option<Value>, &'static str> {
        let id = value["span_id"].as_u64().ok_or("missing span_id")?;
        if name.contains("server_request") {
            // Each poll emits another exit with updated fields. Retain the
            // final exit and count the request only when its span closes.
            self.pending.insert(id, value);
        } else if name.ends_with("SpanCloseEvent") {
            if let Some(mut request) = self.pending.remove(&id) {
                self.closed += 1;
                request["request_index"] = self.closed.into();
                return Ok(Some(request));
            }
        }
        Ok(None)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths: Vec<_> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        return Err("usage: benchmark-trace-reader <segment.0.bin> [segment.1.bin ...]".into());
    }
    let mut requests = Requests::default();
    let mut out = BufWriter::new(io::stdout().lock());
    // A request can cross a segment boundary. Keep state across segments;
    // span IDs can be reused after close, so they are not global request IDs.
    for path in paths {
        let bytes = std::fs::read(&path)?;
        let mut decoder = Decoder::new(&bytes).ok_or("invalid trace header")?;
        let mut error = None;
        decoder.for_each_event(|raw| {
            if error.is_some()
                || !(raw.name.contains("server_request") || raw.name.ends_with("SpanCloseEvent"))
            {
                return;
            }
            let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                let value: Value = raw.deserialize()?;
                if let Some(request) = requests.event(raw.name, value)? {
                    serde_json::to_writer(&mut out, &request)?;
                    writeln!(out)?;
                }
                Ok(())
            })();
            if let Err(err) = result {
                error = Some(err);
            }
        })?;
        if let Some(err) = error {
            return Err(err);
        }
    }
    if !requests.pending.is_empty() {
        return Err(format!("{} unclosed request spans", requests.pending.len()).into());
    }
    if requests.closed == 0 {
        return Err("no closed server_request spans".into());
    }
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn repeated_exits_count_once_and_span_ids_can_be_reused() {
        let mut requests = Requests::default();
        for completed in 1..=2 {
            for elapsed in [10, 20, 30] {
                assert!(requests
                    .event(
                        "SpanExitEvent<server_request>",
                        json!({"span_id": 7, "body_complete_ns": elapsed}),
                    )
                    .unwrap()
                    .is_none());
            }
            let request = requests
                .event("SpanCloseEvent", json!({"span_id": 7}))
                .unwrap()
                .unwrap();
            assert_eq!(request["body_complete_ns"], 30);
            assert_eq!(request["request_index"], completed);
            assert!(requests.pending.is_empty());
        }
    }

    #[test]
    fn interleaved_requests_close_independently() {
        let mut requests = Requests::default();
        requests
            .event("server_request", json!({"span_id": 1, "path": "/a"}))
            .unwrap();
        requests
            .event("server_request", json!({"span_id": 2, "path": "/b"}))
            .unwrap();
        assert!(requests
            .event("SpanCloseEvent", json!({"span_id": 99}))
            .unwrap()
            .is_none());
        let request = requests
            .event("SpanCloseEvent", json!({"span_id": 2}))
            .unwrap()
            .unwrap();
        assert_eq!(request["path"], "/b");
        assert!(requests.pending.contains_key(&1));
    }
}
