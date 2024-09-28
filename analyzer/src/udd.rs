//! Interface to the Debian Ultimate Debian Database (UDD) mirror
use sqlx::{Error, PgPool};

/// Default URL for the UDD mirror
pub const DEFAULT_UDD_URL: &str =
    "postgresql://udd-mirror:udd-mirror@udd-mirror.debian.net:5432/udd";

/// Connect to the UDD mirror
pub async fn connect_udd_mirror() -> Result<PgPool, Error> {
    PgPool::connect(DEFAULT_UDD_URL).await
}
