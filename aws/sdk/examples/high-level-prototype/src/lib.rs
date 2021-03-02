use dynamodb::operation::ListTables;
use dynamodb::output::ListTablesOutput;
use dynamodb::error::ListTablesError;
use aws_hyper::SdkError;
use tower::{BoxError, Service};
use std::task::{Context, Poll};
use std::pin::Pin;
use std::future::Future;
use smithy_http::body::SdkBody;
use std::sync::{Arc, Mutex};
use dynamodb::input::{ListTablesInput, list_tables_input};
use hyper::client::HttpConnector;
use http::Request;
use hyper_tls::HttpsConnector;
use dynamodb::Config;

pub struct DynamoDb {
    conn: aws_hyper::Client<HttpService>,
    conf: dynamodb::Config
}

#[derive(Clone)]
struct HttpService(Arc<Mutex<dyn HttpServiceT>>);

trait HttpServiceT: Send + Sync {
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), BoxError>>;
    fn call(&mut self, req: http::Request<SdkBody>) -> Pin<Box<dyn Future<Output=Result<http::Response<hyper::Body>, BoxError>> + Send>>;
}

impl HttpServiceT for hyper::Client<HttpsConnector<HttpConnector>, SdkBody> {
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), BoxError>> {
        Service::poll_ready(self, cx).map_err(|e|e.into())
    }

    fn call(&mut self, req: http::Request<SdkBody>) -> Pin<Box<dyn Future<Output=Result<http::Response<hyper::Body>, BoxError>> + Send>> {
        let inner = Service::call(self, req);
        let fut = async move {
            inner.await.map_err(|err|err.into())
        };
        Box::pin(fut)
    }
}

impl tower::Service<http::Request<SdkBody>> for HttpService {
    type Response = http::Response<hyper::Body>;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output=Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.lock().unwrap().poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<SdkBody>) -> Self::Future {
        self.0.lock().unwrap().call(req)
    }
}

impl DynamoDb {
    pub async fn list_tables(&self, op: list_tables_input::Builder) -> Result<ListTablesOutput, SdkError<ListTablesError>> {
        self.conn.call(op.build(&self.conf)).await
    }

    pub fn from_env() -> Self {
        let https = HttpsConnector::new();
        let client = hyper::Client::builder().build::<_, SdkBody>(https);
        DynamoDb {
            conf: Config::builder().build(),
            conn: aws_hyper::Client::new(HttpService(Arc::new(Mutex::new(client))))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::DynamoDb;
    use dynamodb::operation::ListTables;

    #[tokio::test]
    async fn list_tables() {
       let client = DynamoDb::from_env();
       let tables = client.list_tables(ListTables::builder()).await.expect("list tables should succeed");
       println!("{:#?}", tables);
    }
}
