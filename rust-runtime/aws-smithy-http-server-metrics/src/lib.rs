#![allow(missing_docs)]

pub mod layer;
pub mod service;

// struct MyMetrics {}

// impl<Init, Req, Res, ReqBody, ResBody, CE, ES> Default
//     for MetricsLayer<Init, Req, Res, ReqBody, ResBody, CE, ES>
// where
//     Init: Fn() -> AppendAndCloseOnDrop<CE, ES> + Clone,
//     CE: CloseEntry,
//     ES: EntrySink<RootEntry<CE::Closed>>,
//     Req: for<'a, 'b> Fn(&'a Request<ReqBody>, &'b mut AppendAndCloseOnDrop<CE, ES>) + Clone,
//     Res: for<'a, 'b> Fn(&'a Response<ResBody>, &'b mut AppendAndCloseOnDrop<CE, ES>) + Clone,
// {
//     fn default() -> Self {
//         RequestMetricsLayerBuilder::default().build()
//     }
// }


// #[derive(Debug)]
// pub struct MetricsPlugin {
//     pub(crate) with_default_request_metrics: bool,
//     pub(crate) with_operation_name_metric: bool,
//     pub(crate) with_service_name_metric: bool,
//     pub(crate) with_service_version_metric: bool,
// }

// impl HttpMarker for MetricsPlugin {}

// impl<Ser, Op, T> Plugin<Ser, Op, T> for MetricsPlugin
// where
//     Op: OperationShape,
//     Ser: ServiceShape,
// {
//     type Output = MetricsOperationService<T>;

//     fn apply(&self, inner: T) -> Self::Output {
//         if !self.with_default_request_metrics {
//             return MetricsOperationService {
//                 inner,
//                 operation_name: None,
//                 service_name: None,
//                 service_version: None,
//             };
//         }

//         let service_name = self.with_service_name_metric.then(|| Ser::ID.name());
//         let service_version = self
//             .with_service_version_metric
//             .then(|| Ser::VERSION)
//             .flatten();
//         let operation_name = self.with_operation_name_metric.then(|| Op::ID.name());

//         MetricsOperationService {
//             inner,
//             operation_name: operation_name,
//             service_name: service_name,
//             service_version: service_version,
//         }
//     }
// }

// #[derive(Debug)]
// pub struct MetricsOperationService<T> {
//     inner: T,
//     operation_name: Option<&'static str>,
//     service_name: Option<&'static str>,
//     service_version: Option<&'static str>,
// }

// impl<T> Clone for MetricsOperationService<T>
// where
//     T: Clone,
// {
//     fn clone(&self) -> Self {
//         Self {
//             inner: self.inner.clone(),
//             operation_name: self.operation_name,
//             service_name: self.service_name,
//             service_version: self.service_version,
//         }
//     }
// }

// impl<T, ReqBody, ResBody> Service<Request<ReqBody>> for MetricsOperationService<T>
// where
//     T: Service<Request<ReqBody>, Response = Response<ResBody>>,
//     T::Future: Send + 'static,
// {
//     type Response = T::Response;
//     type Error = T::Error;
//     type Future = std::pin::Pin<
//         Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>,
//     >;

//     fn call(&mut self, mut request: Request<ReqBody>) -> Self::Future {
//         let request_metrics = request
//             .extensions_mut()
//             .get_mut::<SlotGuard<DefaultRequestMetrics>>();

//         if let Some(request_metrics) = request_metrics {
//             request_metrics.operation_name = self.operation_name.map(|n| n.to_string());
//             request_metrics.service_name = self.service_name.map(|n| n.to_string());
//             request_metrics.service_version = self.service_version.map(|n| n.to_string());
//         }

//         self.inner.call(request).boxed()

//         // TODO, have a fallback where if request metrics is not in the extensions, initialize here and do the same logic
//         // That way, if users want to ignore the layer, they can only use the http plugin. We'll have some helper methods to
//         // dedup / maintain parity, because we need it to be compatible both ways in case someone who only has the plugin
//         // suddenly needs to switch over to using the outer layer middleware to fold their metrics.
//     }

//     fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
//         self.inner.poll_ready(cx)
//     }
// }

// fn try_extract_request_id<B>(request: &Request<B>) -> Option<String> {
//     request
//         .headers()
//         .get("x-amzn-requestid")?
//         .to_str()
//         .inspect_err(|error| {
//             tracing::warn!(
//                 "x-amzn-requestid header was not representable as string: {:?}",
//                 error
//             );
//         })
//         .ok()
//         .map(Into::into)
// }
