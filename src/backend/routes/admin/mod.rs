mod file_operations;

use crate::auth::AppState;
use axum::Router;

pub fn router() -> Router<AppState> {
    file_operations::router()
}
