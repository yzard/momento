mod admin;
mod ai;
mod albums;
mod auth;
mod backup;
mod client;
mod control_json;
mod duplicates;
mod faces;
pub(crate) mod file_stream;
#[path = "import/mod.rs"]
mod imports;
mod map;
mod media;
mod metadata;
mod places;
mod public;
mod share;
mod trash;
mod users;

use crate::auth::AppState;
use axum::Router;

pub(crate) use control_json::{render_json, render_message, CpuJson};
pub use trash::cleanup_expired_trash;

pub fn api_router() -> Router<AppState> {
    Router::new()
        .merge(admin::router())
        .merge(auth::router())
        .merge(client::router())
        .merge(backup::router())
        .merge(ai::router())
        .merge(duplicates::router())
        .merge(faces::router())
        .merge(users::router())
        .merge(media::router())
        .merge(albums::router())
        .merge(map::router())
        .merge(places::router())
        .merge(share::router())
        .merge(public::router())
        .merge(imports::router())
        .merge(metadata::router())
        .merge(trash::router())
}
