use async_trait::async_trait;
use debian_control::lossless::relations::{Relation, Relations};
use debversion::Version;
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;

async fn package_version(
    conn: &PgPool,
    package: &str,
    release: &str,
) -> Result<Option<Version>, sqlx::Error> {
    sqlx::query_scalar::<_, Version>(
        "SELECT version FROM packages WHERE package = $1 AND release = $2",
    )
    .bind(package)
    .bind(release)
    .fetch_optional(conn)
    .await
}

async fn package_provides(
    conn: &PgPool,
    package: &str,
    release: &str,
) -> Result<Option<Vec<Relation>>, sqlx::Error> {
    let provides: Option<String> = sqlx::query_scalar::<_, String>(
        "SELECT provides FROM packages WHERE package = $1 AND release = $2",
    )
    .bind(package)
    .bind(release)
    .fetch_optional(conn)
    .await?;

    if let Some(provides) = provides {
        let rels: Relations = provides.parse().unwrap();

        let mut ret = vec![];
        for entry in rels.entries() {
            ret.push(entry.relations().next().unwrap());
        }
        Ok(Some(ret))
    } else {
        Ok(None)
    }
}

async fn package_essential(
    conn: &PgPool,
    package: &str,
    release: &str,
) -> Result<Option<bool>, sqlx::Error> {
    sqlx::query_scalar::<_, bool>(
        "SELECT (essential = 'yes') FROM packages WHERE package = $1 AND release = $2",
    )
    .bind(package)
    .bind(release)
    .fetch_optional(conn)
    .await
}

async fn package_build_essential(
    conn: &PgPool,
    package: &str,
    release: &str,
) -> Result<bool, sqlx::Error> {
    let rows = sqlx::query_scalar::<_, String>(
        "select depends from packages where package = $1 and release = $2",
    )
    .bind("build-essential")
    .bind(release)
    .fetch_all(conn)
    .await?;

    let mut build_essential = HashSet::new();
    for row in rows {
        let rels: Relations = row.parse().unwrap();
        build_essential.extend(
            rels.entries()
                .flat_map(|e| e.relations().map(|r| r.name()).collect::<Vec<_>>()),
        );
    }

    Ok(build_essential.contains(package))
}

async fn fetch_transitions(conn: &PgPool, release: &str) -> HashMap<String, String> {
    let mut ret = HashMap::new();
    for transition in crate::dummy_transitional::find_dummy_transitional_packages(conn, release)
        .await
        .unwrap()
        .into_values()
    {
        ret.insert(transition.from_name, transition.to_expr.to_string());
    }
    ret
}

pub struct UddPackageChecker {
    release: String,
    build: bool,
    transitions: Arc<Mutex<Option<HashMap<String, String>>>>,
    conn: sqlx::PgPool,
}

impl UddPackageChecker {
    /// Create a new PackageChecker for the given release.
    ///
    /// If `build` is true, the checker will consider build-essential packages
    /// as essential.
    pub async fn new(release: &str, build: bool) -> Self {
        Self {
            release: release.to_string(),
            build,
            transitions: Arc::new(Mutex::new(None)),
            conn: debian_analyzer::udd::connect_udd_mirror().await.unwrap(),
        }
    }
}

#[async_trait]
impl PackageChecker for UddPackageChecker {
    fn release(&self) -> &str {
        &self.release
    }

    async fn package_version(&self, package: &str) -> Result<Option<Version>, sqlx::Error> {
        package_version(&self.conn, package, &self.release).await
    }

    async fn replacement(&self, package: &str) -> Result<Option<String>, sqlx::Error> {
        let mut transitions = self.transitions.lock().await;
        if transitions.is_none() {
            *transitions = Some(fetch_transitions(&self.conn, &self.release).await);
        }
        Ok(transitions
            .as_ref()
            .and_then(|t| t.get(package))
            .map(|x| x.to_string()))
    }

    async fn package_provides(
        &self,
        package: &str,
    ) -> Result<Vec<(String, Option<Version>)>, sqlx::Error> {
        package_provides(&self.conn, package, &self.release)
            .await
            .map(|provides| {
                provides
                    .unwrap_or_default()
                    .into_iter()
                    .map(|rel| (rel.name().to_string(), rel.version().map(|x| x.1)))
                    .collect()
            })
    }

    async fn is_essential(&self, package: &str) -> Result<Option<bool>, sqlx::Error> {
        if self.build && package_build_essential(&self.conn, package, &self.release).await? {
            return Ok(Some(true));
        }
        package_essential(&self.conn, package, &self.release).await
    }
}

#[async_trait]
pub trait PackageChecker {
    fn release(&self) -> &str;

    async fn package_version(&self, package: &str) -> Result<Option<Version>, sqlx::Error>;

    async fn replacement(&self, package: &str) -> Result<Option<String>, sqlx::Error>;

    async fn package_provides(
        &self,
        package: &str,
    ) -> Result<Vec<(String, Option<Version>)>, sqlx::Error>;

    async fn is_essential(&self, package: &str) -> Result<Option<bool>, sqlx::Error>;
}
