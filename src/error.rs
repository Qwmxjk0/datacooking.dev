use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    Unprocessable(String),
    Unavailable(String),
    Internal(String),
}

impl AppError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    pub fn unprocessable(msg: impl Into<String>) -> Self {
        Self::Unprocessable(msg.into())
    }

    pub fn unavailable(msg: impl Into<String>) -> Self {
        Self::Unavailable(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, message.as_str()),
            Self::Unprocessable(message) => (StatusCode::UNPROCESSABLE_ENTITY, message.as_str()),
            Self::Unavailable(message) => (StatusCode::SERVICE_UNAVAILABLE, message.as_str()),
            Self::Internal(message) => (StatusCode::INTERNAL_SERVER_ERROR, message.as_str()),
        };
        (status, message.to_string()).into_response()
    }
}
