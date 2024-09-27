use super::{
  api_model::{
    BaseResponse, ConfigBatchListenRequest, ConfigChangeBatchListenResponse,
    ConfigChangeNotifyRequest, ConfigContext, ConfigQueryRequest, ConfigQueryResponse,
    ServerCheckResponse, CONFIG_MODEL, ERROR_CODE, SUCCESS_CODE,
  },
  nacos_proto::{
    bi_request_stream_server::{BiRequestStream, BiRequestStreamServer},
    request_server::{Request, RequestServer},
    Payload,
  },
  utils::{HandlerResult, PayloadUtils},
};
use crate::config::{provider::ConfigProvider, target::Target};
use lambda_extension::{
  tracing::{debug, warn},
  Error,
};
use lazy_static::lazy_static;
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::{broadcast, mpsc};
use tonic::transport::Server;

pub fn spawn(
  addr: SocketAddr,
  target_tx: mpsc::Sender<(Target, String)>,
  config_tx: broadcast::Sender<(Target, mpsc::Sender<()>)>,
  cp: impl ConfigProvider + 'static,
) {
  tokio::spawn(async move {
    let request_server = RequestServerImpl { cp, target_tx };
    let bi_request_stream_server = BiRequestStreamServerImpl { config_tx };
    Server::builder()
      .add_service(RequestServer::new(request_server))
      .add_service(BiRequestStreamServer::new(bi_request_stream_server))
      .serve(addr)
      .await
      .unwrap();
  });
}

// https://github.com/nacos-group/r-nacos/blob/c74dfab019b0771e38a28be7821b08021afd10c4/src/config/config_type.rs#L3
lazy_static! {
  pub(crate) static ref CONFIG_TYPE_TEXT: Arc<String> = Arc::new("text".to_string());
  // pub(crate) static ref CONFIG_TYPE_JSON: Arc<String> = Arc::new("json".to_string());
  // pub(crate) static ref CONFIG_TYPE_XML: Arc<String> = Arc::new("xml".to_string());
  // pub(crate) static ref CONFIG_TYPE_YAML: Arc<String> = Arc::new("yaml".to_string());
  // pub(crate) static ref CONFIG_TYPE_HTML: Arc<String> = Arc::new("html".to_string());
  // pub(crate) static ref CONFIG_TYPE_PROPERTIES: Arc<String> = Arc::new("properties".to_string());
  // pub(crate) static ref CONFIG_TYPE_TOML: Arc<String> = Arc::new("toml".to_string());
}

// https://github.com/nacos-group/r-nacos/blob/c74dfab019b0771e38a28be7821b08021afd10c4/src/grpc/handler/mod.rs#L44
pub(crate) const HEALTH_CHECK_REQUEST: &str = "HealthCheckRequest";
pub(crate) const SERVER_CHECK_REQUEST: &str = "ServerCheckRequest";
pub(crate) const CONFIG_QUERY_REQUEST: &str = "ConfigQueryRequest";
pub(crate) const CONFIG_BATCH_LISTEN_REQUEST: &str = "ConfigBatchListenRequest";

struct RequestServerImpl<CP> {
  target_tx: mpsc::Sender<(Target, String)>,
  cp: CP,
}

impl<CP: ConfigProvider + Clone + Send + 'static> RequestServerImpl<CP> {
  async fn handle(&self, payload: Payload) -> Result<HandlerResult, Error> {
    let Some(url) = PayloadUtils::get_payload_type(&payload) else {
      // https://github.com/nacos-group/r-nacos/blob/c74dfab019b0771e38a28be7821b08021afd10c4/src/grpc/handler/mod.rs#L237
      return Ok(HandlerResult::error(302u16, "empty type url".to_owned()));
    };

    match url.as_str() {
      HEALTH_CHECK_REQUEST => {
        // https://github.com/nacos-group/r-nacos/blob/c74dfab019b0771e38a28be7821b08021afd10c4/src/grpc/handler/mod.rs#L242
        let response = BaseResponse::build_success_response();
        Ok(HandlerResult::success(PayloadUtils::build_payload(
          "HealthCheckResponse",
          serde_json::to_string(&response)?,
        )))
      }
      SERVER_CHECK_REQUEST => {
        // https://github.com/nacos-group/r-nacos/blob/c74dfab019b0771e38a28be7821b08021afd10c4/src/grpc/handler/mod.rs#L200
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
        let request: ConfigQueryRequest = serde_json::from_slice(&body_vec)?;
        let mut response = ConfigQueryResponse {
          request_id: request.request_id,
          ..Default::default()
        };

        debug!(data_id = %request.data_id, group = %request.group, tenant = %request.tenant, "ConfigQueryRequest");

        match self
          .cp
          .clone()
          .get(
            &request.data_id,
            &request.group,
            if request.tenant.is_empty() {
              None
            } else {
              Some(&request.tenant)
            },
            false,
          )
          .await
        {
          Ok(config) => {
            response.result_code = SUCCESS_CODE;
            response.content = config.content().to_owned().into();
            response.content_type = Some(CONFIG_TYPE_TEXT.clone()); // TODO: use correct content type? does this matter?
            response.last_modified = 0; // TODO: does this matter?
            response.md5 = Some(config.md5().to_owned().into());

            // https://github.com/nacos-group/r-nacos/blob/c74dfab019b0771e38a28be7821b08021afd10c4/src/grpc/handler/config_query.rs#L85
            Ok(HandlerResult::success(PayloadUtils::build_payload(
              "ConfigQueryResponse",
              serde_json::to_string(&response)?,
            )))
          }
          Err(err) => {
            // https://github.com/nacos-group/r-nacos/blob/c74dfab019b0771e38a28be7821b08021afd10c4/src/grpc/handler/config_query.rs#L90
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
        let body_vec = payload.body.unwrap_or_default().value;
        let request: ConfigBatchListenRequest = serde_json::from_slice(&body_vec)?;

        let mut response = ConfigChangeBatchListenResponse {
          request_id: request.request_id,
          ..Default::default()
        };

        for item in request.config_listen_contexts {
          debug!(data_id = %item.data_id, group = %item.group, tenant = %item.tenant, "ConfigBatchListenRequest");
          let target = Target {
            data_id: item.data_id.clone().into(),
            group: item.group.clone().into(),
            tenant: if item.tenant.is_empty() {
              None
            } else {
              Some(item.tenant.clone().into())
            },
          };
          let cache = self
            .cp
            .clone()
            .get(&target.data_id, &target.group, target.tenant(), false)
            .await?;
          if cache.md5() != item.md5.as_str() {
            let obj = ConfigContext {
              data_id: item.data_id.into(),
              group: item.group.into(),
              tenant: item.tenant.into(),
            };
            response.changed_configs.push(obj);
          }
          // register target to target_manager
          self.target_tx.send((target, item.md5.to_string())).await?;
        }

        response.result_code = SUCCESS_CODE;

        // https://github.com/nacos-group/r-nacos/blob/c74dfab019b0771e38a28be7821b08021afd10c4/src/grpc/handler/config_change_batch_listen.rs#L74
        Ok(HandlerResult::success(PayloadUtils::build_payload(
          "ConfigChangeBatchListenResponse",
          serde_json::to_string(&response)?,
        )))
      }
      _ => {
        warn!("InvokerHandler not found for type:{}", url);
        // https://github.com/nacos-group/r-nacos/blob/c74dfab019b0771e38a28be7821b08021afd10c4/src/grpc/handler/mod.rs#L232
        Ok(HandlerResult::error(
          302u16,
          format!("{} RequestHandler Not Found", url),
        ))
      }
    }
  }
}

#[tonic::async_trait]
impl<CP: ConfigProvider + 'static> Request for RequestServerImpl<CP> {
  async fn request(
    &self,
    request: tonic::Request<Payload>,
  ) -> Result<tonic::Response<Payload>, tonic::Status> {
    let payload = request.into_inner();
    let handle_result = self.handle(payload).await;
    match handle_result {
      Ok(res) => Ok(tonic::Response::new(res.payload)),
      Err(e) => {
        // https://github.com/nacos-group/r-nacos/blob/c74dfab019b0771e38a28be7821b08021afd10c4/src/grpc/server.rs#L192
        Ok(tonic::Response::new(PayloadUtils::build_error_payload(
          500u16,
          e.to_string(),
        )))
      }
    }
  }
}

pub struct BiRequestStreamServerImpl {
  config_tx: broadcast::Sender<(Target, mpsc::Sender<()>)>,
}

#[tonic::async_trait]
impl BiRequestStream for BiRequestStreamServerImpl {
  type requestBiStreamStream =
    tokio_stream::wrappers::ReceiverStream<Result<Payload, tonic::Status>>;

  async fn request_bi_stream(
    &self,
    _request: tonic::Request<tonic::Streaming<Payload>>,
  ) -> Result<tonic::Response<Self::requestBiStreamStream>, tonic::Status> {
    let (payload_tx, payload_rx) = tokio::sync::mpsc::channel(10);
    let r_stream = tokio_stream::wrappers::ReceiverStream::new(payload_rx);

    let mut config_rx = self.config_tx.subscribe();
    tokio::spawn(async move {
      // https://github.com/nacos-group/r-nacos/blob/c74dfab019b0771e38a28be7821b08021afd10c4/src/grpc/bistream_manage.rs#L87
      let mut next_request_id = {
        let mut request_id: u64 = 0;
        move || {
          if request_id >= 0x7fff_ffff_ffff_ffff {
            request_id = 0;
          } else {
            request_id += 1;
          }
          request_id.to_string()
        }
      };

      while let Ok((target, changed_tx)) = config_rx.recv().await {
        debug!("notifying config change: {:?}", target);
        // TODO: check if the updated config is one of we are listening
        let request = ConfigChangeNotifyRequest {
          group: target.group,
          data_id: target.data_id,
          tenant: target.tenant.unwrap_or("".to_string().into()),
          request_id: Some(next_request_id()),
          module: Some(CONFIG_MODEL.to_string()),
          ..Default::default()
        };
        // https://github.com/nacos-group/r-nacos/blob/c74dfab019b0771e38a28be7821b08021afd10c4/src/grpc/bistream_manage.rs#L251
        let payload = PayloadUtils::build_payload(
          "ConfigChangeNotifyRequest",
          serde_json::to_string(&request).unwrap(),
        );
        payload_tx.send(Ok(payload)).await.unwrap();
        // notify the lambda extension to sleep
        changed_tx.send(()).await.unwrap();
      }
    });

    Ok(tonic::Response::new(r_stream))
  }
}
