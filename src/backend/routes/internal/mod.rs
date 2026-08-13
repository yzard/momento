mod llm;

use axum::Router;

use crate::auth::AppState;

pub fn router() -> Router<AppState> {
    llm::router()
}
