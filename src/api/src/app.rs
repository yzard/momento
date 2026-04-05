use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use crate::auth::AppState;
use crate::config::Config;
use crate::database::DbPool;
use crate::logging::request_logger;
use crate::routes::api_router;
use crate::webdav::webdav_router;
use crate::VERSION;

#[derive(Serialize)]
struct HealthcheckResponse {
    status: String,
    version: String,
}

async fn healthcheck() -> Json<HealthcheckResponse> {
    Json(HealthcheckResponse {
        status: "healthy".to_string(),
        version: VERSION.to_string(),
    })
}

pub fn create_app(config: Arc<Config>, pool: DbPool) -> Router {
    let state = AppState {
        config: config.clone(),
        pool,
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api_routes = Router::new()
        .route("/healthcheck", get(healthcheck))
        .merge(api_router());

    let mut app = Router::new()
        .nest("/api/v1", api_routes)
        .merge(webdav_router(state.clone()))
        .layer(middleware::from_fn(request_logger))
        .layer(cors)
        .with_state(state);

    // Serve static files if frontend exists
    let static_dir = std::env::var("MOMENTO_STATIC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("../web/dist"));

    if static_dir.exists() {
        app = app.fallback(move |req: Request<Body>| {
            let static_dir = static_dir.clone();
            async move {
                let path = req.uri().path().trim_start_matches('/');

                if path.starts_with("webdav") {
                    return (StatusCode::NOT_FOUND, "Not Found").into_response();
                }

                // Try to serve static file
                let file_path = static_dir.join(path);
                if file_path.exists() && file_path.is_file() {
                    return serve_static_file(file_path).await;
                }

                // Try assets directory
                if path.starts_with("assets/") {
                    let asset_path = static_dir.join(path);
                    if asset_path.exists() && asset_path.is_file() {
                        return serve_static_file(asset_path).await;
                    }
                }

                // Fall back to index.html for SPA routing
                let index_path = static_dir.join("index.html");
                if index_path.exists() {
                    return serve_static_file(index_path).await;
                }

                (StatusCode::NOT_FOUND, "Not Found").into_response()
            }
        });
    }

    app
}

async fn serve_static_file(path: PathBuf) -> Response {
    match tokio::fs::read(&path).await {
        Ok(contents) => {
            let mime_type = mime_guess::from_path(&path)
                .first_or_octet_stream()
                .to_string();

            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", mime_type)
                .body(Body::from(contents))
                .unwrap()
        }
        Err(_) => (StatusCode::NOT_FOUND, "Not Found").into_response(),
    }
}
