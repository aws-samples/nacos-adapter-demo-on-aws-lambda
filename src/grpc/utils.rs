//! The content of this file is copied from
//! https://github.com/nacos-group/r-nacos/blob/6bb9ab85f7adccdc3bacf59dbda8c32a5daaafd8/src/grpc/mod.rs#L25
#![allow(dead_code)]

use super::{api_model::BaseResponse, nacos_proto};
use std::collections::HashMap;

pub struct HandlerResult {
  pub success: bool,
  pub payload: nacos_proto::Payload,
  pub message: Option<String>,
}
impl HandlerResult {
  pub fn success(payload: nacos_proto::Payload) -> Self {
    Self {
      success: true,
      message: None,
      payload,
    }
  }

  pub fn error(code: u16, message: String) -> Self {
    let payload = PayloadUtils::build_error_payload(code, message.clone());
    Self {
      success: false,
      message: Some(message),
      payload,
    }
  }

  pub fn error_mark(payload: nacos_proto::Payload) -> Self {
    Self {
      success: false,
      message: None,
      payload,
    }
  }

  pub fn error_with_message(payload: nacos_proto::Payload, message: String) -> Self {
    Self {
      success: false,
      message: Some(message),
      payload,
    }
  }
}

pub struct PayloadUtils;

impl PayloadUtils {
  pub fn new_metadata(
    r#type: &str,
    client_ip: &str,
    headers: HashMap<String, String>,
  ) -> nacos_proto::Metadata {
    nacos_proto::Metadata {
      r#type: r#type.to_owned(),
      client_ip: client_ip.to_owned(),
      headers,
    }
  }

  pub fn build_error_payload(error_code: u16, error_msg: String) -> nacos_proto::Payload {
    let error_val = BaseResponse::build_error_response(error_code, error_msg).to_json_string();
    Self::build_payload("ErrorResponse", error_val)
  }

  pub fn build_payload(url: &str, val: String) -> nacos_proto::Payload {
    Self::build_full_payload(url, val, "", Default::default())
  }

  pub fn build_full_payload(
    url: &str,
    val: String,
    client_ip: &str,
    headers: HashMap<String, String>,
  ) -> nacos_proto::Payload {
    let body = nacos_proto::Any {
      type_url: "".into(),
      value: val.into_bytes(),
    };
    let meta = Self::new_metadata(url, client_ip, headers);
    nacos_proto::Payload {
      body: Some(body),
      metadata: Some(meta),
    }
  }

  pub fn get_payload_header(payload: &nacos_proto::Payload) -> String {
    let mut str = String::default();
    if let Some(meta) = &payload.metadata {
      str.push_str(&format!("type:{},", meta.r#type));
      str.push_str(&format!("client_ip:{},", meta.client_ip));
      str.push_str(&format!("header:{:?},", meta.headers));
    }
    str
  }

  pub fn get_payload_string(payload: &nacos_proto::Payload) -> String {
    let mut str = String::default();
    if let Some(meta) = &payload.metadata {
      str.push_str(&format!("type:{},\n\t", meta.r#type));
      str.push_str(&format!("client_ip:{},\n\t", meta.client_ip));
      str.push_str(&format!("header:{:?},\n\t", meta.headers));
    }
    if let Some(body) = &payload.body {
      let new_value = body.clone();
      let value_str = String::from_utf8(new_value.value).unwrap();
      str.push_str(&format!("body:{}", value_str));
    }
    str
  }

  pub fn get_payload_type(payload: &nacos_proto::Payload) -> Option<&String> {
    if let Some(meta) = &payload.metadata {
      return Some(&meta.r#type);
    }
    None
  }
}
