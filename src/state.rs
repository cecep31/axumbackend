use crate::infra::{database::DbPool, email::Mailer};
use axum::extract::FromRef;
use std::sync::Arc;

/// Shared application dependencies, built once in `main` and handed to the
/// router with `with_state`. Handlers keep extracting only what they need
/// (`State<DbPool>`, `State<Arc<dyn Mailer>>`) through `FromRef`.
#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,
    pub mailer: Arc<dyn Mailer>,
}

impl FromRef<AppState> for DbPool {
    fn from_ref(state: &AppState) -> Self {
        state.db.clone()
    }
}

impl FromRef<AppState> for Arc<dyn Mailer> {
    fn from_ref(state: &AppState) -> Self {
        state.mailer.clone()
    }
}
