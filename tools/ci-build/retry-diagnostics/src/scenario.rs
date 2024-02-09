/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Serialize)]
pub(crate) struct Scenario {
    pub(crate) name: String,
    pub(crate) response: ScenarioResponse,
}

#[derive(Debug, Serialize)]
pub(crate) enum ScenarioResponse {
    Timeout,
    Response {
        status_code: u16,
        headers: HashMap<String, String>,
        body: String,
    },
}

impl ScenarioResponse {
    pub(crate) fn status_code(&self) -> u16 {
        match self {
            ScenarioResponse::Timeout => 0,
            ScenarioResponse::Response { status_code, .. } => *status_code,
        }
    }
}

pub(crate) mod dynamodb {
    use crate::scenario::{set_connection_close, Scenario, ScenarioResponse};
    use std::collections::HashMap;

    const DYNAMO_THROTTLING_RESPONSE: &str = r#"{"__type":"com.amazonaws.dynamodb.v20120810#ThrottlingException", "message":"enhance your calm"}"#;

    pub(crate) fn setup() -> Scenario {
        Scenario {
            name: "setup".into(),
            response: ScenarioResponse::Response {
                status_code: 200,
                headers: HashMap::new(),
                body: DYNAMO_THROTTLING_RESPONSE.into(),
            },
        }
    }

    pub(crate) fn dynamo_scenario(status_code: u16, error_code: Option<&str>) -> Scenario {
        let name = error_code.unwrap_or("NO RESPONSE BODY").to_string();

        let body = match error_code {
            Some(error_code) => format!(
                r#"{{"__type":"com.amazonaws.dynamodb.v20120810#{error_code}", "message":"enhance your calm"}}"#
            ),
            None => "".to_string(),
        };
        Scenario {
            name,
            response: ScenarioResponse::Response {
                status_code,
                headers: HashMap::new(),
                body,
            },
        }
    }

    pub(crate) fn dynamo_throttling_429() -> Scenario {
        dynamo_scenario(429, Some("ThrottlingException"))
    }

    pub(crate) fn dynamo_throttling_500() -> Scenario {
        dynamo_scenario(500, Some("ThrottlingException"))
    }

    pub(crate) fn dynamo_throttling_503() -> Scenario {
        dynamo_scenario(503, Some("ThrottlingException"))
    }

    pub(crate) fn throttling_with_close_header() -> Scenario {
        set_connection_close(dynamo_scenario(503, Some("ThrottlingException")))
    }

    pub(crate) fn timeout() -> Scenario {
        Scenario {
            name: "timeout".into(),
            response: ScenarioResponse::Timeout,
        }
    }

    pub(crate) fn empty_body_400() -> Scenario {
        dynamo_scenario(400, None)
    }
}

pub(crate) fn set_connection_close(mut resp: Scenario) -> Scenario {
    let ScenarioResponse::Response { headers, .. } = &mut resp.response else {
        return resp;
    };
    headers.insert("Connection".into(), "close".into());
    resp.name += "(close header set)";
    resp
}

pub(crate) fn set_keepalive(mut resp: Scenario) -> Scenario {
    let ScenarioResponse::Response { headers, .. } = &mut resp.response else {
        return resp;
    };
    headers.insert("Connection".into(), "Keep-Alive".into());
    resp.name += "(keep-alive header set)";
    resp
}

pub(crate) mod s3 {
    use crate::scenario::{Scenario, ScenarioResponse};
    use std::collections::HashMap;

    pub(crate) fn s3_error(code: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Error>
  <Code>{code}</Code>
  <Message>some message</Message>
  <Resource>/mybucket/myfoto.jpg</Resource>
  <RequestId>4442587FB7D0A2F9</RequestId>
</Error>"#
        )
    }

    pub(crate) fn setup() -> Scenario {
        Scenario {
            name: "setup".into(),
            response: ScenarioResponse::Response {
                status_code: 200,
                headers: HashMap::new(),
                body: "ok!".to_string(),
            },
        }
    }

    pub(crate) fn s3_scenario(status_code: u16, code: Option<&str>) -> Scenario {
        let name = code.unwrap_or("NO RESPONSE BODY").to_string();

        let body = match code {
            Some(code) => s3_error(code),
            None => "".to_string(),
        };
        Scenario {
            name,
            response: ScenarioResponse::Response {
                status_code,
                headers: HashMap::new(),
                body,
            },
        }
    }
}
