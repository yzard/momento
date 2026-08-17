mod ai;
mod albums;
mod auth;
mod deduplicate;
mod faces;
#[path = "import/mod.rs"]
mod imports;
mod map;
mod media;
mod metadata;
mod public;
mod share;
mod trash;
mod users;

use crate::auth::AppState;
use axum::Router;

pub use trash::cleanup_expired_trash;

pub fn api_router() -> Router<AppState> {
    Router::new()
        .merge(auth::router())
        .merge(ai::router())
        .merge(deduplicate::router())
        .merge(faces::router())
        .merge(users::router())
        .merge(media::router())
        .merge(media::thumbnail_router())
        .merge(media::preview_router())
        .merge(albums::router())
        .merge(map::router())
        .merge(share::router())
        .merge(public::router())
        .merge(imports::router())
        .merge(metadata::router())
        .merge(trash::router())
}
