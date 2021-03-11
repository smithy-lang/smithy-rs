use aws_hyper::SdkError;
use dynamodb::error::ListTablesError;
use dynamodb::input::{list_tables_input, ListTablesInput};
use dynamodb::operation::ListTables;
use dynamodb::output::ListTablesOutput;
use dynamodb::Config;
use http::Request;
use hyper::client::HttpConnector;
use hyper_tls::HttpsConnector;
use smithy_http::body::SdkBody;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tower::{BoxError, Service};
use aws_hyper::conn::Standard;

pub struct DynamoDb {
    conn: Arc<aws_hyper::Client<Standard>>,
    conf: Arc<dynamodb::Config>,
}

pub struct ListTablesFluentBuilder {
    inner: list_tables_input::Builder,
    conf: Arc<dynamodb::Config>,
    conn: Arc<aws_hyper::Client<Standard>>
}

impl ListTablesFluentBuilder {
    fn new(conf: Arc<dynamodb::Config>, conn: Arc<aws_hyper::Client<Standard>>) -> Self {
        ListTablesFluentBuilder {
            conf,
            conn,
            inner: Default::default()
        }
    }
    /// <p>The first table name that this operation will evaluate. Use the value that was returned for
    /// <code>LastEvaluatedTableName</code> in a previous operation, so that you can obtain the next page
    /// of results.</p>
    pub fn exclusive_start_table_name(mut self, inp: impl Into<::std::string::String>) -> Self {
        self.inner = self.inner.exclusive_start_table_name(inp);
        self
    }
    /// <p>A maximum number of table names to return. If this parameter is not specified, the limit is 100.</p>
    pub fn limit(mut self, inp: i32) -> Self {
        self.inner = self.inner.limit(inp);
        self
    }

    pub async fn execute(self) -> Result<ListTablesOutput, SdkError<ListTablesError>> {
        let op = self.inner.build(&self.conf);
        self.conn.call(op).await
    }
}

impl DynamoDb {
    pub fn list_tables(
        &self,
    ) -> ListTablesFluentBuilder {
        ListTablesFluentBuilder::new(self.conf.clone(), self.conn.clone())
    }

    pub fn from_env() -> Self {
        DynamoDb {
            conf: Arc::new(Config::builder().build()),
            conn: Arc::new(aws_hyper::Client::https())
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
        let tables = client
            .list_tables()
            .limit(10)
            .exclusive_start_table_name("start_table")
            .execute().await;
        println!("{:#?}", tables);
    }
}
