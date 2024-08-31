use sqlx::{Error, FromRow, PgPool, Row};

pub const DEFAULT_UDD_URL: &str =
    "postgresql://udd-mirror:udd-mirror@udd-mirror.debian.net:5432/udd";

pub async fn connect_udd_mirror() -> Result<PgPool, Error> {
    PgPool::connect(DEFAULT_UDD_URL).await
}
