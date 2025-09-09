
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

mod backup_routes;
mod cursor;
mod org_to_cell_mapping;
mod types;

use org_to_cell_mapping::{Command, OrgToCell};
use types::Cell;

#[derive(Serialize)]
struct ApiResponse {
    cell: Option<String>,
    locality: Option<String>,
}

impl IntoResponse for ApiResponse {
    fn into_response(self) -> Response {
        (StatusCode::OK, Json(self)).into_response()
    }
}

impl From<Option<Cell>> for ApiResponse {
    fn from(maybe_cell: Option<Cell>) -> Self {
        match maybe_cell {
            Some(cell) => ApiResponse {
                cell: Some((*cell.id).clone()),
                locality: Some((*cell.locality).clone()),
            },
            None => ApiResponse {
                cell: None,
                locality: None,
            },
        }
    }
}

#[derive(Serialize)]
struct ApiErrorResponse {
    error_message: String,
}

impl IntoResponse for ApiErrorResponse {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, Json(self)).into_response()
    }
}

#[derive(Deserialize, Debug)]
struct Params {
    org_id: String,
    locality: Option<String>,
}

pub fn run() {
    if tokio::runtime::Handle::try_current().is_ok() {
        println!("Already inside a tokio runtime, use run_async() directly");
        return;
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(run_async());
}

pub async fn run_async() {
    // Dummy data for testing. The real provider implementation should be selected based on config.
    let route_provider = backup_routes::PlaceholderRouteProvider {};

    let routes = OrgToCell::new(route_provider);

    // Channel to send commands to the worker thread.
    let (_cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);

    // Spawn the loader thread. All loading should happen from this thread.
    let routes_clone = routes.clone();
    tokio::spawn(async move {
        routes_clone.run_loader_worker(cmd_rx).await;
    });

    let app = Router::new().route("/", get(handler)).with_state(routes);
    let listener = TcpListener::bind("127.0.0.1:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn handler(
    State(org_to_cell): State<OrgToCell>,
    Query(params): Query<Params>,
) -> Result<ApiResponse, ApiErrorResponse> {
    let cell = org_to_cell.lookup(&params.org_id, params.locality.as_deref());

    match cell {
        Ok(maybe_cell) => Ok(maybe_cell.into()),
        Err(e) => {
            eprintln!("Error looking up cell: {e}");
            Err(ApiErrorResponse {
                error_message: e.to_string(),
            })
        }
    }
}
