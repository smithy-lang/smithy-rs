use aws_sigv4::{sign, Credentials, Region, RequestExt, SignedService};
use dynamo::model::{
    AttributeDefinition, KeySchemaElement,
    KeyType, ProvisionedThroughput, ScalarAttributeType,
};
use dynamo::operation;
use http_body;
use http_body::Body;
use hyper::body::Buf;
use hyper::client::HttpConnector;
use hyper::http::request;
use hyper::{Client, Request, Uri, Response};
use std::error::Error;

/// macro to execute an AWS request, currently required because no traits exist
/// to specify the required methods.
///
/// # Example
/// ```rust
/// let hyper_client: Client<HttpConnector, hyper::Body> = hyper::Client::builder().build_http();
/// let clear_tables = operation::DeleteTable::builder().table_name("my_table").build();
/// let cleared = make_request!(hyper_client, clear_tables);
/// ```
macro_rules! make_request {
    ($client:expr, $input:expr) => {{
        let inp = $input;
        let request = inp.to_http_request();
        let request = prepare_request(request);
        let response = $client
            .request(request)
            .await?;
        let response = prepare_response(response).await?;
        inp.parse_response(response)
    }};
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let hyper_client: Client<HttpConnector, hyper::Body> = hyper::Client::builder().build_http();
    let create_table_input = operation::CreateTable::builder()
        .table_name("my_table")
        .key_schema(vec![KeySchemaElement::builder()
            .attribute_name("str")
            .key_type(KeyType::from("HASH"))
            .build()])
        .attribute_definitions(vec![AttributeDefinition::builder()
            .attribute_name("str")
            .attribute_type(ScalarAttributeType::from("S"))
            .build()])
        .provisioned_throughput(
            ProvisionedThroughput::builder()
                .read_capacity_units(100)
                .write_capacity_units(100)
                .build(),
        )
        .build();

    let clear_tables = operation::DeleteTable::builder()
        .table_name("my_table")
        .build();
    let resp = make_request!(hyper_client, &clear_tables);
    let resp = make_request!(hyper_client, clear_tables);
    println!("table cleared: {:?}", resp);

    let create_table = make_request!(hyper_client, create_table_input).unwrap();
    println!(
        "table created: {:?}",
        create_table.table_description.unwrap().table_name.unwrap()
    );
    let tables = make_request!(
        hyper_client,
        operation::ListTables::builder().limit(5).build()
    ).unwrap();
    println!("tables : {:?}", &tables);
    Ok(())
}

async fn prepare_response(response: Response<hyper::Body>) -> Result<Response<Vec<u8>>, <hyper::Body as http_body::Body>::Error> {
    let (parts, body) = response.into_parts();
    let data = read_body(body).await?;
    Ok(Response::from_parts(parts, data))
}

fn prepare_request(mut request: request::Request<Vec<u8>>) -> Request<hyper::Body> {
    let uri = Uri::builder()
        .authority("localhost:8000")
        .scheme("http")
        .path_and_query(
            request
                .uri()
                .path_and_query()
                .unwrap()
                .clone(),
        )
        .build()
        .expect("valid uri");

    (*request.uri_mut()) = uri;

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
