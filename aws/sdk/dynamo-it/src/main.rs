/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::error::Error;

use dynamodb::output::{DeleteTableOutput, ListTablesOutput};
use operation::{SdkBody, Operation, ParseStrictResponse, Request};
use dynamodb::error::DeleteTableError;

struct DeleteTable(dynamodb::operation::DeleteTable);

//use bytes::Bytes;
//use auth::{SigningConfig, ServiceConfig, RequestConfig};
//use std::time::SystemTime;
//use operation::endpoint::StaticEndpoint;
use http::{Response, Uri};
use dynamodb::operation::CreateTable;
use dynamodb::model::{AttributeDefinition, ScalarAttributeType, KeySchemaElement, ProvisionedThroughput, KeyType};
use bytes::Bytes;
use auth::{RequestConfig, ServiceConfig};
use std::time::SystemTime;


impl ParseStrictResponse for DeleteTable {
    type Output = Result<DeleteTableOutput, DeleteTableError>;
    fn parse(&self, response: &Response<Bytes>) -> Self::Output {
        self.0.parse_response(response)
    }
}

use auth::SigningConfig;
use operation::endpoint::{StaticEndpoint, ProvideEndpoint, EndpointProviderExt};
use std::sync::Arc;
use operation::signing_middleware::{SigningConfigExt, CredentialProviderExt};

impl DeleteTable {
    fn into_operation(self, config: &dynamodb::Config) -> Operation<DeleteTable> {
        let mut request = operation::Request::new(self.0.build_http_request().map(|body|SdkBody::from(body)));
        request.config.insert_signing_config(SigningConfig::default_config(
            ServiceConfig {
                service: "dynamodb".into(),
                region: "us-east-1".into()
            },
            RequestConfig {
                request_ts: ||SystemTime::now()
            },
        ));
        request.config.insert_credentials_provider(config.credentials_provider.clone());
        let endpoint_config: Arc<dyn ProvideEndpoint> = Arc::new(StaticEndpoint::from_uri(Uri::from_static("http://localhost:8000")));
        request.config.insert_endpoint_provider(endpoint_config);
        Operation {
            request,
            response_handler: Box::new(self)
        }
    }
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let table_name = "new_table";
    let config = dynamodb::Config::builder().build();
    let client = aws_hyper::Client::default();
    let delete_table = dynamodb::operation::DeleteTable::builder()
        .table_name(table_name)
        .build(&config);
    let clear_table = DeleteTable(delete_table).into_operation(&config);
    let response = client.call(clear_table).await;
    match response {
        Ok(output) => println!("deleted! {:?}", output.parsed),
        Err(e) => println!("err: {:?}", e.error())
    }
    Ok(())
}
