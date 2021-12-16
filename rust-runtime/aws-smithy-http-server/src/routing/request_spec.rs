/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use http::Request;
use regex::Regex;

#[derive(Debug, Clone)]
pub enum PathSegment {
    Literal(String),
    Label,
    Greedy,
}

#[derive(Debug, Clone)]
pub enum QuerySegment {
    Key(String),
    KeyValue(String, String),
}

#[derive(Debug, Clone)]
pub enum HostPrefixSegment {
    Literal(String),
    Label,
}

#[derive(Debug, Clone, Default)]
pub struct PathSpec(Vec<PathSegment>);

impl PathSpec {
    pub fn from_vector_unchecked(path_segments: Vec<PathSegment>) -> Self {
        PathSpec(path_segments)
    }
}

#[derive(Debug, Clone, Default)]
pub struct QuerySpec(Vec<QuerySegment>);

impl QuerySpec {
    pub fn from_vector_unchecked(query_segments: Vec<QuerySegment>) -> Self {
        QuerySpec(query_segments)
    }
}

#[derive(Debug, Clone, Default)]
pub struct PathAndQuerySpec {
    pub path_segments: PathSpec,
    pub query_segments: QuerySpec,
}

#[derive(Debug, Clone)]
pub struct UriSpec {
    pub host_prefix: Option<Vec<HostPrefixSegment>>,
    pub path_and_query: PathAndQuerySpec,
}

#[derive(Debug, Clone)]
pub struct RequestSpec {
    method: http::Method,
    uri_spec: UriSpec,
    uri_path_regex: Regex,
}

#[derive(Debug, PartialEq)]
pub enum Match {
    /// The request matches the URI pattern spec.
    Yes,
    /// The request matches the URI pattern spec, but the wrong HTTP method was used. `405 Method
    /// Not Allowed` should be returned in the response.
    MethodNotAllowed,
    /// The request does not match the URI pattern spec. `404 Not Found` should be returned in the
    /// response.
    No,
}

impl From<&PathSpec> for Regex {
    fn from(uri_path_spec: &PathSpec) -> Self {
        let sep = "/+";
        let re = uri_path_spec
            .0
            .iter()
            .map(|segment_spec| match segment_spec {
                PathSegment::Literal(literal) => literal,
                // TODO(https://github.com/awslabs/smithy/issues/975) URL spec says it should be ASCII but this regex accepts UTF-8:
                PathSegment::Label => "[^/]+",
                PathSegment::Greedy => ".*",
            })
            .fold(String::new(), |a, b| a + sep + b);

        Regex::new(&format!("{}$", re)).unwrap()
    }
}

impl RequestSpec {
    pub fn new(method: http::Method, uri_spec: UriSpec) -> Self {
        let uri_path_regex = (&uri_spec.path_and_query.path_segments).into();
        RequestSpec {
            method,
            uri_spec,
            uri_path_regex,
        }
    }

    pub(super) fn matches<B>(&self, req: &Request<B>) -> Match {
        if let Some(_host_prefix) = &self.uri_spec.host_prefix {
            todo!("Look at host prefix");
        }

        if !self.uri_path_regex.is_match(req.uri().path()) {
            return Match::No;
        }

        if self.uri_spec.path_and_query.query_segments.0.is_empty() {
            if self.method == req.method() {
                return Match::Yes;
            } else {
                return Match::MethodNotAllowed;
            }
        }

        match req.uri().query() {
            Some(query) => {
                // We can't use `HashMap<&str, &str>` because a query string key can appear more
                // than once e.g. `/?foo=bar&foo=baz`. We _could_ use a multiset e.g. the `hashbag`
                // crate.
                let res = serde_urlencoded::from_str::<Vec<(&str, &str)>>(query);

                match res {
                    Err(_) => Match::No,
                    Ok(query_map) => {
                        for query_segment in self.uri_spec.path_and_query.query_segments.0.iter() {
                            match query_segment {
                                QuerySegment::Key(key) => {
                                    if !query_map.iter().any(|(k, _v)| k == key) {
                                        return Match::No;
                                    }
                                }
                                QuerySegment::KeyValue(key, expected_value) => {
                                    let mut it = query_map.iter().filter(|(k, _v)| k == key).peekable();
                                    if it.peek().is_none() {
                                        return Match::No;
                                    }

                                    // The query key appears more than once. All of its values must
                                    // coincide and be equal to the expected value.
                                    if it.any(|(_k, v)| v != expected_value) {
                                        return Match::No;
                                    }
                                }
                            }
                        }

                        if self.method == req.method() {
                            Match::Yes
                        } else {
                            Match::MethodNotAllowed
                        }
                    }
                }
            }
            None => Match::No,
        }
    }

    // Helper function to build a `RequestSpec`.
    #[cfg(test)]
    pub fn from_parts(
        method: http::Method,
        path_segments: Vec<PathSegment>,
        query_segments: Vec<QuerySegment>,
    ) -> Self {
        Self::new(
            method,
            UriSpec {
                host_prefix: None,
                path_and_query: PathAndQuerySpec {
                    path_segments: PathSpec::from_vector_unchecked(path_segments),
                    query_segments: QuerySpec::from_vector_unchecked(query_segments),
                },
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::req;
    use super::*;
    use http::Method;

    #[test]
    fn greedy_labels_match_greedily() {
        let spec = RequestSpec::from_parts(
            Method::GET,
            vec![
                PathSegment::Literal(String::from("mg")),
                PathSegment::Greedy,
                PathSegment::Literal(String::from("z")),
            ],
            vec![],
        );

        let hits = vec![
            (Method::GET, "/mg/a/z"),
            (Method::GET, "/mg/z/z"),
            (Method::GET, "/mg/a/z/b/z"),
            (Method::GET, "/mg/a/z/z/z"),
        ];
        for (method, uri) in &hits {
            assert_eq!(Match::Yes, spec.matches(&req(method, uri)));
        }
    }

    #[test]
    fn repeated_query_keys() {
        let spec = RequestSpec::from_parts(Method::DELETE, vec![], vec![QuerySegment::Key(String::from("foo"))]);

        let hits = vec![
            (Method::DELETE, "/?foo=bar&foo=bar"),
            (Method::DELETE, "/?foo=bar&foo=baz"),
            (Method::DELETE, "/?foo&foo"),
        ];
        for (method, uri) in &hits {
            assert_eq!(Match::Yes, spec.matches(&req(method, uri)));
        }
    }

    fn key_value_spec() -> RequestSpec {
        RequestSpec::from_parts(
            Method::DELETE,
            vec![],
            vec![QuerySegment::KeyValue(String::from("foo"), String::from("bar"))],
        )
    }

    #[test]
    fn repeated_query_keys_same_values_match() {
        assert_eq!(
            Match::Yes,
            key_value_spec().matches(&req(&Method::DELETE, "/?foo=bar&foo=bar"))
        );
    }

    #[test]
    fn repeated_query_keys_distinct_values_does_not_match() {
        assert_eq!(
            Match::No,
            key_value_spec().matches(&req(&Method::DELETE, "/?foo=bar&foo=baz"))
        );
    }

    fn ab_spec() -> RequestSpec {
        RequestSpec::from_parts(
            Method::GET,
            vec![
                PathSegment::Literal(String::from("a")),
                PathSegment::Literal(String::from("b")),
            ],
            vec![],
        )
    }

    #[test]
    fn empty_segments_in_the_middle_dont_matter() {
        let hits = vec![
            (Method::GET, "/a/b"),
            (Method::GET, "/a//b"),
            (Method::GET, "//////a//b"),
        ];
        for (method, uri) in &hits {
            assert_eq!(Match::Yes, ab_spec().matches(&req(method, uri)));
        }
    }

    // The rationale is that `/index` points to the `index` resource, but `/index/` points to "the
    // default resource under `index`", for example `/index/index.html`, so trailing slashes at the
    // end of URIs _do_ matter.
    #[test]
    fn empty_segments_at_the_end_do_matter() {
        let misses = vec![
            (Method::GET, "/a/b/"),
            (Method::GET, "/a/b//"),
            (Method::GET, "//a//b////"),
        ];
        for (method, uri) in &misses {
            assert_eq!(Match::No, ab_spec().matches(&req(method, uri)));
        }
    }
}
