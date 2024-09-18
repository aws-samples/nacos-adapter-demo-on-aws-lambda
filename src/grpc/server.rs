use crate::config::provider::ConfigProvider;

use super::{
  api_model::{
    BaseResponse, ConfigQueryRequest, ConfigQueryResponse, ServerCheckResponse, ERROR_CODE,
    NOT_FOUND, SUCCESS_CODE,
  },
  nacos_proto::{
    self,
    bi_request_stream_server::BiRequestStream,
    request_server::{Request, RequestServer},
    Payload,
  },
  utils::{HandlerResult, PayloadUtils},
};
use lambda_extension::tracing::{error, info, warn};
use lazy_static::lazy_static;
use std::{net::SocketAddr, sync::Arc};
use tonic::transport::Server;

lazy_static! {
  pub(crate) static ref CONFIG_TYPE_TEXT: Arc<String> = Arc::new("text".to_string());
  pub(crate) static ref CONFIG_TYPE_JSON: Arc<String> = Arc::new("json".to_string());
  pub(crate) static ref CONFIG_TYPE_XML: Arc<String> = Arc::new("xml".to_string());
  pub(crate) static ref CONFIG_TYPE_YAML: Arc<String> = Arc::new("yaml".to_string());
  pub(crate) static ref CONFIG_TYPE_HTML: Arc<String> = Arc::new("html".to_string());
  pub(crate) static ref CONFIG_TYPE_PROPERTIES: Arc<String> = Arc::new("properties".to_string());
  pub(crate) static ref CONFIG_TYPE_TOML: Arc<String> = Arc::new("toml".to_string());
}

pub(crate) const HEALTH_CHECK_REQUEST: &str = "HealthCheckRequest";
pub(crate) const SERVER_CHECK_REQUEST: &str = "ServerCheckRequest";
pub(crate) const CONFIG_QUERY_REQUEST: &str = "ConfigQueryRequest";
pub(crate) const CONFIG_BATCH_LISTEN_REQUEST: &str = "ConfigBatchListenRequest";

pub struct RequestServerImpl<Config> {
  cp: Config,
}

impl<Config: ConfigProvider + Clone + Send + 'static> RequestServerImpl<Config> {
  pub async fn handle(&self, payload: Payload) -> anyhow::Result<HandlerResult> {
    let Some(url) = PayloadUtils::get_payload_type(&payload) else {
      return Ok(HandlerResult::error(302u16, "empty type url".to_owned()));
    };

    match url.as_str() {
      HEALTH_CHECK_REQUEST => {
        let response = BaseResponse::build_success_response();
        Ok(HandlerResult::success(PayloadUtils::build_payload(
          "HealthCheckResponse",
          serde_json::to_string(&response)?,
        )))
      }
      SERVER_CHECK_REQUEST => {
        let response = ServerCheckResponse {
          result_code: SUCCESS_CODE,
          connection_id: Some("".to_owned()),
          ..Default::default()
        };
        Ok(HandlerResult::success(PayloadUtils::build_payload(
          "ServerCheckResponse",
          serde_json::to_string(&response)?,
        )))
      }
      CONFIG_QUERY_REQUEST => {
        let body_vec = payload.body.unwrap_or_default().value;
        let request: ConfigQueryRequest = serde_json::from_slice(&body_vec).unwrap();
        let mut response = ConfigQueryResponse {
          request_id: request.request_id,
          ..Default::default()
        };

        info!(data_id = %request.data_id, group = %request.group, tenant = %request.tenant, "ConfigQueryRequest");

        match self
          .cp
          .clone()
          .get(&request.data_id, &request.group, Some(&request.tenant))
          .await
        {
          Ok(config) => {
            response.result_code = SUCCESS_CODE;
            response.content = config.content().to_owned().into();
            response.content_type = Some(CONFIG_TYPE_TEXT.clone());
            response.last_modified = 0;
            response.md5 = Some(config.md5().to_owned().into());
            Ok(HandlerResult::success(PayloadUtils::build_payload(
              "ConfigQueryResponse",
              serde_json::to_string(&response)?,
            )))
          }
          Err(err) => {
            response.result_code = ERROR_CODE;
            response.error_code = ERROR_CODE;
            response.message = Some(err.to_string());
            Ok(HandlerResult::success(PayloadUtils::build_payload(
              "ErrorResponse",
              serde_json::to_string(&response)?,
            )))
          }
        }
      }
      CONFIG_BATCH_LISTEN_REQUEST => {
        todo!()
      }
      _ => {
        warn!("InvokerHandler not fund handler,type:{}", url);
        Ok(HandlerResult::error(
          302u16,
          format!("{} RequestHandler Not Found", url),
        ))
      }
    }
  }
}

pub fn spawn(addr: SocketAddr, cp: impl ConfigProvider + Clone + Send + 'static) {
  tokio::spawn(async move {
    let request_server = RequestServerImpl { cp };
    // let bi_request_stream_server =
    //     BiRequestStreamServerImpl::new(grpc_app_data.bi_stream_manage.clone());
    Server::builder()
      .add_service(RequestServer::new(request_server))
      // .add_service(BiRequestStreamServer::new(bi_request_stream_server))
      .serve(addr)
      .await
      .unwrap();
  });
}

#[tonic::async_trait]
impl<Config: ConfigProvider + Clone + Send + 'static> Request for RequestServerImpl<Config> {
  async fn request(
    &self,
    request: tonic::Request<Payload>,
  ) -> Result<tonic::Response<Payload>, tonic::Status> {
    let payload = request.into_inner();
    let handle_result = self.handle(payload).await;
    match handle_result {
      Ok(res) => Ok(tonic::Response::new(res.payload)),
      Err(e) => Ok(tonic::Response::new(PayloadUtils::build_error_payload(
        500u16,
        e.to_string(),
      ))),
    }
  }
}

// pub struct BiRequestStreamServerImpl {}

// #[tonic::async_trait]
// impl BiRequestStream for BiRequestStreamServerImpl {
//   type requestBiStreamStream =
//     tokio_stream::wrappers::ReceiverStream<Result<Payload, tonic::Status>>;

//   async fn request_bi_stream(
//     &self,
//     request: tonic::Request<tonic::Streaming<Payload>>,
//   ) -> Result<tonic::Response<Self::requestBiStreamStream>, tonic::Status> {
//     todo!()
//   }
// }
