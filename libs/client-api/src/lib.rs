mod http;

#[cfg(feature = "collab-sync")]
pub mod collab_sync;

pub mod notify;
pub mod ws;

pub use http::*;

pub mod error {
  pub use shared_entity::app_error::AppError;
  pub use shared_entity::error_code::ErrorCode;
}

// Export all dto entities that will be used in the frontend application
pub mod entity {
  pub use database_entity::dto::*;
  pub use gotrue_entity::dto::*;
  pub use shared_entity::dto::*;
}
