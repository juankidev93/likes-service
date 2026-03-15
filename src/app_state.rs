use sqlx::PgPool;

#[allow(dead_code)]
#[derive(Clone)]
pub struct AppState {
    pub db_pool: PgPool,
}
