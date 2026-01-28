use thiserror::Error;

#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum CarrefourError {
    #[error("Not authenticated. Run 'carrefour auth set --session <SESSID>' first.")]
    NotAuthenticated,

    #[error("Session expired or invalid. Please re-authenticate with a fresh session cookie.")]
    Unauthorized,

    #[error("Product not found: {0}")]
    ProductNotFound(String),

    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Rate limited. Please wait before making more requests.")]
    RateLimited,

    #[error("Cart is empty")]
    EmptyCart,

    #[error("API error ({status}): {message}")]
    ApiError { status: u16, message: String },

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Failed to parse response: {0}")]
    ParseError(String),
}

impl CarrefourError {
    pub fn from_status(status: u16, body: &str) -> Self {
        match status {
            401 => CarrefourError::Unauthorized,
            404 => CarrefourError::NotFound(body.to_string()),
            429 => CarrefourError::RateLimited,
            _ => CarrefourError::ApiError {
                status,
                message: body.to_string(),
            },
        }
    }
}
