use aws_sigv4::{sign, Credentials, Region, RequestExt, SignedService};
use dynamo::model::{AttributeDefinition, AttributeValue, CreateTableOutput, DeleteTableOutput, GetItemOutput, KeySchemaElement, KeyType, ProvisionedThroughput, PutItemOutput, ScalarAttributeType, QueryOutput, ListTablesOutput};
use dynamo::operation;
use http_body;
use http_body::Body;
use hyper::body::Buf;
use hyper::client::HttpConnector;
use hyper::http::request;
use hyper::{Client, Request, Uri};
use serde::de::DeserializeOwned;
use smithy_types::Blob;
use std::collections::HashMap;
use std::error::Error;

/// macro to execute an AWS request, currently required because no traits exist
/// to specify the required methods.
///
/// # Example
/// ```rust
/// let hyper_client: Client<HttpConnector, hyper::Body> = hyper::Client::builder().build_http();
/// let clear_tables = operation::DeleteTableInput::builder().table_name("my_table").build();
/// let cleared = make_request!(hyper_client, clear_tables, DeleteTableOutput);
/// ```
macro_rules! make_request {
    ($client:expr, $input:expr, $out:ty) => {{
        let inp = $input;
        let response = $client
            .request(prepare_request(
                inp.request_builder_base(),
                inp.build_body(),
            ))
            .await?;
        let body = read_body(response.into_body()).await?;
        //println!("{}", std::str::from_utf8(&body).unwrap());
        from_response::<$out>(body)
    }};
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let hyper_client: Client<HttpConnector, hyper::Body> = hyper::Client::builder().build_http();
    let create_table_input = operation::CreateTableInput::builder()
        .table_name("my_table")
        .key_schema(vec![
            KeySchemaElement::builder()
                .attribute_name("str")
                .key_type(KeyType::from("HASH"))
                .build(),
        ])
        .attribute_definitions(vec![
            AttributeDefinition::builder()
                .attribute_name("str")
                .attribute_type(ScalarAttributeType::from("S"))
                .build(),
        ])
        .provisioned_throughput(
            ProvisionedThroughput::builder()
                .read_capacity_units(100)
                .write_capacity_units(100)
                .build(),
        )
        .build();

    let clear_tables = operation::DeleteTableInput::builder()
        .table_name("my_table")
        .build();
    let _ = make_request!(hyper_client, clear_tables, DeleteTableOutput);
    let create_table = make_request!(hyper_client, create_table_input, CreateTableOutput);
    println!("table created: {:?}", create_table.table_description.unwrap().table_name.unwrap());
    let tables = make_request!(hyper_client, operation::ListTablesInput::builder().build(), ListTablesOutput);
    println!("tables : {:?}", &tables);
    Ok(())
}

fn from_response<T: DeserializeOwned>(response: Vec<u8>) -> T {
    serde_json::from_slice(response.as_slice()).expect("deserialization failed")
}

fn prepare_request(request_builder: request::Builder, body: Vec<u8>) -> Request<hyper::Body> {
    let uri = Uri::builder()
        .authority("localhost:8000")
        .scheme("http")
        .path_and_query(
            request_builder
                .uri_ref()
                .unwrap()
                .path_and_query()
                .unwrap()
                .clone(),
        )
        .build()
        .expect("valid uri");

    let mut request: Request<Vec<u8>> = request_builder.uri(uri).body(body).unwrap();
    request.set_region(Region::new("us-east-1"));
    request.set_service(SignedService::new("dynamodb"));
    sign(&mut request, &Credentials::new("asdf", "asdf", None)).unwrap();
    request.map(|body| body.into())
}

async fn read_body<B: http_body::Body>(body: B) -> Result<Vec<u8>, B::Error> {
    let mut output = Vec::new();
    pin_utils::pin_mut!(body);
    while let Some(buf) = body.data().await {
        let buf = buf?;
        if buf.has_remaining() {
            output.extend_from_slice(buf.bytes())
        }
    }
    Ok(output)
}
