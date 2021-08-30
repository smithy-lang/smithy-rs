/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use http::Uri;

pub struct QueryWriter {
    base_uri: Uri,
    new_path_and_query: String,
    prefix: Option<char>,
}

impl QueryWriter {
    pub fn new(uri: &Uri) -> Self {
        let new_path_and_query = uri
            .path_and_query()
            .map(|pq| pq.to_string())
            .unwrap_or_default();
        let prefix = if uri.query().is_none() {
            Some('?')
        } else if !uri.query().unwrap_or_default().is_empty() {
            Some('&')
        } else {
            None
        };
        QueryWriter {
            base_uri: uri.clone(),
            new_path_and_query,
            prefix,
        }
    }

    pub fn insert(&mut self, k: &str, v: &str) {
        if let Some(prefix) = self.prefix {
            self.new_path_and_query.push(prefix);
        }
        self.prefix = Some('&');
        self.new_path_and_query
            .push_str(&smithy_http::query::fmt_string(k));
        self.new_path_and_query.push('=');

        self.new_path_and_query
            .push_str(&smithy_http::query::fmt_string(v));
    }

    pub fn build(self) -> Uri {
        let mut parts = self.base_uri.into_parts();
        parts.path_and_query = Some(
            self.new_path_and_query
                .parse()
                .expect("adding query should not invalidate URI"),
        );
        Uri::from_parts(parts).expect("a valid URL in should always produce a valid URL out")
    }
}

#[cfg(test)]
mod test {
    use crate::query_writer::QueryWriter;
    use http::Uri;

    #[test]
    fn empty_uri() {
        let uri = Uri::from_static("http://www.example.com");
        let mut query_writer = QueryWriter::new(&uri);
        query_writer.insert("key", "val%ue");
        query_writer.insert("another", "value");
        assert_eq!(
            query_writer.build(),
            Uri::from_static("http://www.example.com?key=val%25ue&another=value")
        );
    }

    #[test]
    fn uri_with_path() {
        let uri = Uri::from_static("http://www.example.com/path");
        let mut query_writer = QueryWriter::new(&uri);
        query_writer.insert("key", "val%ue");
        query_writer.insert("another", "value");
        assert_eq!(
            query_writer.build(),
            Uri::from_static("http://www.example.com/path?key=val%25ue&another=value")
        );
    }

    #[test]
    fn uri_with_path_and_query() {
        let uri = Uri::from_static("http://www.example.com/path?original=here");
        let mut query_writer = QueryWriter::new(&uri);
        query_writer.insert("key", "val%ue");
        query_writer.insert("another", "value");
        assert_eq!(
            query_writer.build(),
            Uri::from_static(
                "http://www.example.com/path?original=here&key=val%25ue&another=value"
            )
        );
    }
}
