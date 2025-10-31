/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::borrow::Cow;

use crate::http;

use crate::http::Request;
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
    path_segments: PathSpec,
    query_segments: QuerySpec,
}

impl PathAndQuerySpec {
    pub fn new(path_segments: PathSpec, query_segments: QuerySpec) -> Self {
        PathAndQuerySpec {
            path_segments,
            query_segments,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UriSpec {
    host_prefix: Option<Vec<HostPrefixSegment>>,
    path_and_query: PathAndQuerySpec,
}

impl UriSpec {
    // TODO(https://github.com/smithy-lang/smithy-rs/issues/950): When we add support for the endpoint
    // trait, this constructor will take in a first argument `host_prefix`.
    pub fn new(path_and_query: PathAndQuerySpec) -> Self {
        UriSpec {
            host_prefix: None,
            path_and_query,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RequestSpec {
    method: http::Method,
    uri_spec: UriSpec,
    uri_path_regex: Regex,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Match {
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
        let sep = "/";
        let re = if uri_path_spec.0.is_empty() {
            String::from(sep)
        } else {
            uri_path_spec
                .0
                .iter()
                .map(|segment_spec| match segment_spec {
                    PathSegment::Literal(literal) => Cow::Owned(regex::escape(literal)),
                    // TODO(https://github.com/awslabs/smithy/issues/975) URL spec says it should be ASCII but this regex accepts UTF-8:
                    // `*` instead of `+` because the empty string `""` can be bound to a label.
                    PathSegment::Label => Cow::Borrowed("[^/]*"),
                    PathSegment::Greedy => Cow::Borrowed(".*"),
                })
                .fold(String::new(), |a, b| a + sep + &b)
        };

        Regex::new(&format!("^{}$", re)).expect("invalid `Regex` from `PathSpec`; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues")
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

    /// A measure of how "important" a `RequestSpec` is. The more specific a `RequestSpec` is, the
    /// higher it ranks in importance. Specificity is measured by the number of segments plus the
    /// number of query string literals in its URI pattern, so `/{Bucket}/{Key}?query` is more
    /// specific than `/{Bucket}/{Key}`, which is more specific than `/{Bucket}`, which is more
    /// specific than `/`.
    ///
    /// This rank effectively induces a total order, but we don't implement as `Ord` for
    /// `RequestSpec` because it would appear in its public interface.
    ///
    /// # Why do we need this?
    ///
    /// Note that:
    ///     1. the Smithy spec does not define how servers should route incoming requests in the
    ///        case of pattern conflicts; and
    ///     2. the Smithy spec even outright rejects conflicting patterns that can be easily
    ///        disambiguated e.g. `/{a}` and `/{label}/b` cannot coexist.
    ///
    /// We can't to anything about (2) since the Smithy CLI will refuse to build a model with those
    /// kind of conflicts. However, the Smithy CLI does allow _other_ conflicting patterns to
    /// coexist, e.g. `/` and `/{label}`. We therefore have to take a stance on (1), since if we
    /// route arbitrarily [we render basic usage
    /// impossible](https://github.com/smithy-lang/smithy-rs/issues/1009).
    /// So this ranking of routes implements some basic pattern conflict disambiguation with some
    /// common sense. It's also the same behavior that [the TypeScript sSDK is implementing].
    ///
    /// [the TypeScript sSDK is implementing]: https://github.com/awslabs/smithy-typescript/blob/d263078b81485a6a2013d243639c0c680343ff47/smithy-typescript-ssdk-libs/server-common/src/httpbinding/mux.ts#L59.
    // TODO(https://github.com/awslabs/smithy/issues/1029#issuecomment-1002683552): Once Smithy
    // updates the spec to define the behavior, update our implementation.
    pub(crate) fn rank(&self) -> usize {
        self.uri_spec.path_and_query.path_segments.0.len() + self.uri_spec.path_and_query.query_segments.0.len()
    }

    pub(crate) fn matches<B>(&self, req: &Request<B>) -> Match {
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
                // We can't use `HashMap<Cow<str>, Cow<str>>` because a query string key can appear more
                // than once e.g. `/?foo=bar&foo=baz`. We _could_ use a multiset e.g. the `hashbag`
                // crate.
                // We must deserialize into `Cow<str>`s because `serde_urlencoded` might need to
                // return an owned allocated `String` if it has to percent-decode a slice of the query string.
                let res = serde_urlencoded::from_str::<Vec<(Cow<str>, Cow<str>)>>(query);

                match res {
                    Err(error) => {
                        tracing::debug!(query, %error, "failed to deserialize query string");
                        Match::No
                    }
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
    use super::*;
    use crate::protocol::test_helpers::req;

    use http::Method;

    #[test]
    fn path_spec_into_regex() {
        let cases = vec![
            (PathSpec(vec![]), "^/$"),
            (PathSpec(vec![PathSegment::Literal(String::from("a"))]), "^/a$"),
            (
                PathSpec(vec![PathSegment::Literal(String::from("a")), PathSegment::Label]),
                "^/a/[^/]*$",
            ),
            (
                PathSpec(vec![PathSegment::Literal(String::from("a")), PathSegment::Greedy]),
                "^/a/.*$",
            ),
            (
                PathSpec(vec![
                    PathSegment::Literal(String::from("a")),
                    PathSegment::Greedy,
                    PathSegment::Literal(String::from("suffix")),
                ]),
                "^/a/.*/suffix$",
            ),
        ];

        for case in cases {
            let re: Regex = (&case.0).into();
            assert_eq!(case.1, re.as_str());
        }
    }

    #[test]
    fn paths_must_match_spec_from_the_beginning_literal() {
        let spec = RequestSpec::from_parts(
            Method::GET,
            vec![PathSegment::Literal(String::from("path"))],
            Vec::new(),
        );

        let misses = vec![(Method::GET, "/beta/path"), (Method::GET, "/multiple/stages/in/path")];
        for (method, uri) in &misses {
            assert_eq!(Match::No, spec.matches(&req(method, uri, None)));
        }
    }

    #[test]
    fn paths_must_match_spec_from_the_beginning_label() {
        let spec = RequestSpec::from_parts(Method::GET, vec![PathSegment::Label], Vec::new());

        let misses = vec![
            (Method::GET, "/prefix/label"),
            (Method::GET, "/label/suffix"),
            (Method::GET, "/prefix/label/suffix"),
        ];
        for (method, uri) in &misses {
            assert_eq!(Match::No, spec.matches(&req(method, uri, None)));
        }
    }

    #[test]
    fn greedy_labels_match_greedily() {
        let spec = RequestSpec::from_parts(
            Method::GET,
            vec![
                PathSegment::Literal(String::from("mg")),
                PathSegment::Greedy,
                PathSegment::Literal(String::from("z")),
            ],
            Vec::new(),
        );

        let hits = vec![
            (Method::GET, "/mg/a/z"),
            (Method::GET, "/mg/z/z"),
            (Method::GET, "/mg/a/z/b/z"),
            (Method::GET, "/mg/a/z/z/z"),
        ];
        for (method, uri) in &hits {
            assert_eq!(Match::Yes, spec.matches(&req(method, uri, None)));
        }
    }

    #[test]
    fn repeated_query_keys() {
        let spec = RequestSpec::from_parts(Method::DELETE, Vec::new(), vec![QuerySegment::Key(String::from("foo"))]);

        let hits = vec![
            (Method::DELETE, "/?foo=bar&foo=bar"),
            (Method::DELETE, "/?foo=bar&foo=baz"),
            (Method::DELETE, "/?foo&foo"),
        ];
        for (method, uri) in &hits {
            assert_eq!(Match::Yes, spec.matches(&req(method, uri, None)));
        }
    }

    fn key_value_spec() -> RequestSpec {
        RequestSpec::from_parts(
            Method::DELETE,
            Vec::new(),
            vec![QuerySegment::KeyValue(String::from("foo"), String::from("bar"))],
        )
    }

    #[test]
    fn repeated_query_keys_same_values_match() {
        assert_eq!(
            Match::Yes,
            key_value_spec().matches(&req(&Method::DELETE, "/?foo=bar&foo=bar", None))
        );
    }

    #[test]
    fn repeated_query_keys_distinct_values_does_not_match() {
        assert_eq!(
            Match::No,
            key_value_spec().matches(&req(&Method::DELETE, "/?foo=bar&foo=baz", None))
        );
    }

    #[test]
    fn encoded_query_string() {
        let request_spec =
            RequestSpec::from_parts(Method::DELETE, Vec::new(), vec![QuerySegment::Key("foo".to_owned())]);

        assert_eq!(
            Match::Yes,
            request_spec.matches(&req(&Method::DELETE, "/?foo=hello%20world", None))
        );
    }

    fn ab_spec() -> RequestSpec {
        RequestSpec::from_parts(
            Method::GET,
            vec![
                PathSegment::Literal(String::from("a")),
                PathSegment::Literal(String::from("b")),
            ],
            Vec::new(),
        )
    }

    // Empty segments _have meaning_ and should not be stripped away when doing routing or label
    // extraction.
    // See https://github.com/awslabs/smithy/issues/1024 for discussion.

    #[test]
    fn empty_segments_in_the_middle_do_matter() {
        assert_eq!(Match::Yes, ab_spec().matches(&req(&Method::GET, "/a/b", None)));

        let misses = vec![(Method::GET, "/a//b"), (Method::GET, "//////a//b")];
        for (method, uri) in &misses {
            assert_eq!(Match::No, ab_spec().matches(&req(method, uri, None)));
        }
    }

    #[test]
    fn empty_segments_in_the_middle_do_matter_label_spec() {
        let label_spec = RequestSpec::from_parts(
            Method::GET,
            vec![
                PathSegment::Literal(String::from("a")),
                PathSegment::Label,
                PathSegment::Literal(String::from("b")),
            ],
            Vec::new(),
        );

        let hits = vec![
            (Method::GET, "/a/label/b"),
            (Method::GET, "/a//b"), // Label is bound to `""`.
        ];
        for (method, uri) in &hits {
            assert_eq!(Match::Yes, label_spec.matches(&req(method, uri, None)));
        }

        assert_eq!(Match::No, label_spec.matches(&req(&Method::GET, "/a///b", None)));
    }

    #[test]
    fn empty_segments_in_the_middle_do_matter_greedy_label_spec() {
        let greedy_label_spec = RequestSpec::from_parts(
            Method::GET,
            vec![
                PathSegment::Literal(String::from("a")),
                PathSegment::Greedy,
                PathSegment::Literal(String::from("suffix")),
            ],
            Vec::new(),
        );

        let hits = vec![
            (Method::GET, "/a//suffix"),
            (Method::GET, "/a///suffix"),
            (Method::GET, "/a///a//b///suffix"),
        ];
        for (method, uri) in &hits {
            assert_eq!(Match::Yes, greedy_label_spec.matches(&req(method, uri, None)));
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
            assert_eq!(Match::No, ab_spec().matches(&req(method, uri, None)));
        }
    }

    #[test]
    fn empty_segments_at_the_end_do_matter_label_spec() {
        let label_spec = RequestSpec::from_parts(
            Method::GET,
            vec![PathSegment::Literal(String::from("a")), PathSegment::Label],
            Vec::new(),
        );

        let misses = vec![(Method::GET, "/a"), (Method::GET, "/a//"), (Method::GET, "/a///")];
        for (method, uri) in &misses {
            assert_eq!(Match::No, label_spec.matches(&req(method, uri, None)));
        }

        // In the second example, the label is bound to `""`.
        let hits = vec![(Method::GET, "/a/label"), (Method::GET, "/a/")];
        for (method, uri) in &hits {
            assert_eq!(Match::Yes, label_spec.matches(&req(method, uri, None)));
        }
    }

    #[test]
    fn unsanitary_path() {
        let spec = RequestSpec::from_parts(
            Method::GET,
            vec![
                PathSegment::Literal(String::from("ReDosLiteral")),
                PathSegment::Label,
                PathSegment::Literal(String::from("(a+)+")),
            ],
            Vec::new(),
        );

        assert_eq!(
            Match::Yes,
            spec.matches(&req(&Method::GET, "/ReDosLiteral/abc/(a+)+", None))
        );
    }
}
