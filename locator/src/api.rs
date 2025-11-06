use crate::backup_routes::BackupRouteProvider;
use crate::config::{ControlPlane as ControlPlaneConfig, Listener as ListenerConfig};
use crate::locator::{Locator, LocatorError};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpListener;

#[derive(thiserror::Error, Debug)]
pub enum LocatorApiError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub async fn serve(
    listener: ListenerConfig,
    control_plane: ControlPlaneConfig,
    provider: Arc<dyn BackupRouteProvider + 'static>,
    locality_to_default_cell: Option<HashMap<String, String>>,
) -> Result<(), LocatorApiError> {
    let locator = Locator::new(control_plane.url, provider, locality_to_default_cell);
    let app = Router::new().route("/", get(handler)).with_state(locator);

    let addr = format!("{}:{}", listener.host, listener.port);

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

#[derive(Serialize)]
struct ApiResponse {
    cell: String,
}

impl IntoResponse for ApiResponse {
    fn into_response(self) -> Response {
        (StatusCode::OK, Json(self)).into_response()
    }
}

impl From<String> for ApiResponse {
    fn from(cell: String) -> Self {
        ApiResponse { cell }
    }
}

#[derive(Serialize)]
struct ApiErrorResponse {
    error_message: String,
}

#[derive(Deserialize, Debug)]
struct Params {
    org_id: String,
    locality: Option<String>,
}

async fn handler(
    State(locator): State<Locator>,
    Query(params): Query<Params>,
) -> Result<ApiResponse, LocatorError> {
    locator
        .lookup(&params.org_id, params.locality.as_deref())
        .await
        .map(|cell| cell.into())
}

impl IntoResponse for LocatorError {
    fn into_response(self) -> Response {
        let status = match self {
            LocatorError::NoCell => StatusCode::NOT_FOUND,
            LocatorError::LocalityMismatch {
                requested: _,
                actual: _,
            } => StatusCode::NOT_FOUND,
            LocatorError::NotReady => StatusCode::SERVICE_UNAVAILABLE,
            LocatorError::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
        };

        let body = Json(ApiErrorResponse {
            error_message: self.to_string(),
        });

        (status, body).into_response()
    }
}
