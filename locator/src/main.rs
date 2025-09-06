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

mod org_to_cell_mapping;
use org_to_cell_mapping::{Cell, Command, OrgToCell};

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

#[tokio::main]
async fn main() {
    let routes = OrgToCell::new();

    // Channel to send comamnds to the worker thread.
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);

    // Spawn the loader thread. All loading should happen from this thread.
    let routes_clone = routes.clone();
    tokio::spawn(async move {
        routes_clone.run_loader_worker(cmd_rx).await;
    });

    println!("Loading placeholder data.");
    routes.load_placeholder_data().await;
    println!("Placeholder data loaded.");

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
