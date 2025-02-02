use database_entity::error::DatabaseError;
use std::fmt::Display;
use std::num::ParseIntError;
use std::time::SystemTimeError;
use std::{borrow::Cow, str};

use serde::{Deserialize, Serialize};
use serde_json::Error;

use crate::error_code::ErrorCode;

#[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
pub struct AppError {
  pub code: ErrorCode,
  pub message: Cow<'static, str>,
}

impl AppError {
  pub fn new(code: ErrorCode, message: impl Into<Cow<'static, str>>) -> Self {
    Self {
      code,
      message: message.into(),
    }
  }

  pub fn with_message(&self, message: impl Into<Cow<'static, str>>) -> Self {
    Self {
      code: self.code,
      message: message.into(),
    }
  }

  pub fn is_record_not_found(&self) -> bool {
    self.code == ErrorCode::RecordNotFound
  }
}

impl Display for AppError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!("code:{} msg: {}", self.code, self.message))
  }
}

#[cfg(feature = "cloud")]
impl actix_web::error::ResponseError for AppError {
  fn status_code(&self) -> actix_web::http::StatusCode {
    actix_web::http::StatusCode::OK
  }

  fn error_response(&self) -> actix_web::HttpResponse {
    actix_web::HttpResponse::Ok().json(self)
  }
}
impl From<anyhow::Error> for AppError {
  fn from(err: anyhow::Error) -> Self {
    match err.downcast::<AppError>() {
      Err(err) => AppError::new(ErrorCode::Unhandled, err.to_string()),
      Ok(err) => err,
    }
  }
}

impl From<DatabaseError> for AppError {
  fn from(value: DatabaseError) -> Self {
    match value {
      DatabaseError::RecordNotFound(msg) => AppError::new(ErrorCode::RecordNotFound, msg),
      DatabaseError::UnexpectedData(msg) => {
        AppError::new(ErrorCode::InvalidRequestParams, msg.to_string())
      },
      DatabaseError::NotEnoughPermissions(msg) => {
        AppError::new(ErrorCode::NotEnoughPermissions, msg)
      },
      DatabaseError::StorageSpaceNotEnough => {
        AppError::new(ErrorCode::StorageSpaceNotEnough, value)
      },
      _ => AppError::new(ErrorCode::DBError, value),
    }
  }
}

impl From<gotrue_entity::error::GoTrueError> for AppError {
  fn from(err: gotrue_entity::error::GoTrueError) -> Self {
    match (err.code, err.msg.as_str()) {
      (400, m) if m.starts_with("oauth error") => AppError::new(ErrorCode::OAuthError, err.msg),
      (401, _) => AppError::new(ErrorCode::OAuthError, err.msg),
      (422, _) => AppError::new(ErrorCode::InvalidRequestParams, err.msg),
      _ => AppError::new(
        ErrorCode::Unhandled,
        format!(
          "gotrue error: {}, message: {}, id: {:?}",
          err.code, err.msg, err.error_id,
        ),
      ),
    }
  }
}

#[cfg(feature = "cloud")]
impl From<gotrue_entity::error::OAuthError> for AppError {
  fn from(err: gotrue_entity::error::OAuthError) -> Self {
    AppError::new(ErrorCode::OAuthError, err.to_string())
  }
}

impl From<ErrorCode> for AppError {
  fn from(value: ErrorCode) -> Self {
    AppError::new(value, value.to_string())
  }
}

#[cfg(feature = "cloud")]
impl From<sqlx::types::uuid::Error> for AppError {
  fn from(err: sqlx::types::uuid::Error) -> Self {
    AppError::new(ErrorCode::Unhandled, format!("uuid error: {}", err))
  }
}

#[cfg(feature = "cloud")]
impl From<sqlx::Error> for AppError {
  fn from(err: sqlx::Error) -> Self {
    AppError::new(ErrorCode::Unhandled, format!("sqlx error: {}", err))
  }
}

impl From<reqwest::Error> for AppError {
  fn from(value: reqwest::Error) -> Self {
    AppError::new(ErrorCode::Unhandled, value.to_string())
  }
}

impl From<serde_json::Error> for AppError {
  fn from(value: Error) -> Self {
    AppError::new(ErrorCode::Unhandled, value.to_string())
  }
}

impl From<opener::OpenError> for AppError {
  fn from(value: opener::OpenError) -> Self {
    AppError::new(ErrorCode::OpenError, value.to_string())
  }
}

#[cfg(feature = "cloud")]
impl From<validator::ValidationErrors> for AppError {
  fn from(value: validator::ValidationErrors) -> Self {
    AppError::new(ErrorCode::InvalidRequestParams, value.to_string())
  }
}

impl From<url::ParseError> for AppError {
  fn from(value: url::ParseError) -> Self {
    AppError::new(ErrorCode::InvalidUrl, value.to_string())
  }
}

impl From<ParseIntError> for AppError {
  fn from(value: ParseIntError) -> Self {
    AppError::new(ErrorCode::InvalidUrl, value.to_string())
  }
}

impl From<SystemTimeError> for AppError {
  fn from(value: SystemTimeError) -> Self {
    AppError::new(ErrorCode::Unhandled, value.to_string())
  }
}

impl From<std::io::Error> for AppError {
  fn from(value: std::io::Error) -> Self {
    AppError::new(ErrorCode::IO, value.to_string())
  }
}

#[cfg(feature = "cloud")]
impl From<s3::error::S3Error> for AppError {
  fn from(value: s3::error::S3Error) -> Self {
    AppError::new(ErrorCode::S3Error, value.to_string())
  }
}
