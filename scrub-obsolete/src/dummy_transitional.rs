use debian_control::lossless::relations::{Entry, Relations};
use serde::Serialize;
use sqlx::{PgPool, Row};
use std::collections::{HashMap, HashSet};

lazy_static::lazy_static! {
    pub static ref REGEXES: Vec<regex::Regex> = vec![
        regex::Regex::new(r".*\((.*, )?(dummy )?transitional (dummy )?package\)").unwrap(),
        regex::Regex::new(r".*\((.*, )?(dummy )?transitional (dummy )?package for ([^ ]+)\)").unwrap(),
        regex::Regex::new(r".*\(transitional development files\)").unwrap(),
        regex::Regex::new(r".*\(transitional\)").unwrap(),
        regex::Regex::new(r".* [-—] transitional( package)?").unwrap(),
        regex::Regex::new(r".*\[transitional package\]").unwrap(),
        regex::Regex::new(r".* - transitional (dummy )?package").unwrap(),
        regex::Regex::new(r"transitional package -- safe to remove").unwrap(),
        regex::Regex::new(r"(dummy )?transitional (dummy )?package (for|to) (.*)").unwrap(),
        regex::Regex::new(r"transitional dummy package").unwrap(),
        regex::Regex::new(r"transitional dummy package: ([^ ]+)").unwrap(),
        regex::Regex::new(r"transitional package, ([^ ]+)").unwrap(),
        regex::Regex::new(r"(dummy )?transitional (dummy )?package, ([^ ]+) to ([^ ]+)").unwrap(),
        regex::Regex::new(r"transitional package( [^ ]+)?").unwrap(),
        regex::Regex::new(r"([^ ]+) transitional package").unwrap(),
        regex::Regex::new(r".* transitional package").unwrap(),
        regex::Regex::new(r".*transitional package for .*").unwrap(),
    ];
}

#[derive(Debug)]
pub struct TransitionalPackage {
    pub from_name: String,
    pub to_expr: String,
}

impl Serialize for TransitionalPackage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("from_name", &self.from_name)?;
        map.serialize_entry("to_expr", &self.to_expr)?;
        map.end()
    }
}

pub async fn find_reverse_dependencies(
    udd: &PgPool,
    package: &str,
) -> Result<HashMap<String, HashSet<String>>, sqlx::Error> {
    let mut by_source = HashMap::new();
    let fields = &[
        "recommends",
        "depends",
        "pre_depends",
        "enhances",
        "suggests",
        "provides",
    ];

    let mut builder = sqlx::QueryBuilder::new("SELECT source, package, ");

    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            builder.push(", ");
        }
        builder.push(field);
    }

    builder.push(" FROM packages WHERE ");

    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            builder.push(" OR ");
        }

        builder.push(&format!("{} LIKE CONCAT('%', ", field));
        builder.push_bind(package);
        builder.push("::text, '%%')");
    }

    let query = builder.build();

    for row in query.fetch_all(udd).await? {
        let source: String = row.get("source");
        let binary: String = row.get("package");
        for field in fields {
            let value: String = row.get(field);
            let parsed: Relations = value.parse().unwrap();
            for entry in parsed.entries() {
                for rel in entry.relations() {
                    if rel.name() == package {
                        by_source
                            .entry(source.clone())
                            .or_insert_with(HashSet::new)
                            .insert(binary.clone());
                    }
                }
            }
        }
    }

    let fields = &[
        "build_depends",
        "build_depends_indep",
        "build_depends_arch",
        "build_conflicts",
        "build_conflicts_indep",
        "build_conflicts_arch",
    ];

    let mut builder = sqlx::QueryBuilder::new("SELECT source, ");
    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            builder.push(", ");
        }
        builder.push(field);
    }
    builder.push(" FROM sources WHERE ");
    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            builder.push(" OR ");
        }
        builder.push(&format!("{} LIKE CONCAT('%', ", field));
        builder.push_bind(package);
        builder.push("::text, '%%')");
    }
    let query = builder.build();

    for row in query.fetch_all(udd).await? {
        let source: String = row.get("source");
        for field in fields {
            let value: String = row.get(field);
            let parsed: Relations = value.parse().unwrap();
            for option in parsed.entries() {
                for rel in option.relations() {
                    if rel.name() == package {
                        by_source.entry(source.clone()).or_insert_with(HashSet::new);
                    }
                }
            }
        }
    }
    Ok(by_source)
}

pub async fn find_dummy_transitional_packages(
    udd: &PgPool,
    release: &str,
) -> Result<HashMap<String, TransitionalPackage>, sqlx::Error> {
    let mut ret = HashMap::new();

    let rows = sqlx::query_as::<_, (String, String, Option<String>)>(
        r#"
        SELECT package, description, depends
        FROM packages
        WHERE release = $1 AND description LIKE '%transitional%'
        "#,
    )
    .bind(release)
    .fetch_all(udd)
    .await?;

    for row in rows {
        let r = if let Some(regex) = REGEXES.iter().find(|regex| regex.is_match(&row.1)) {
            regex
        } else {
            log::debug!("Unknown syntax for dummy package description: {:?}", row.1);
            continue;
        };
        log::debug!("{}: {:?}", row.0, r);
        if let Some(depends) = row.2 {
            let depends: Relations = depends.parse().unwrap();
            let mut entries = depends.entries();
            let e = if let Some(e) = entries.next() {
                e
            } else {
                Entry::new()
            };
            if entries.next().is_some() {
                log::debug!("no single transition target for {}: {:?}", row.0, depends);
                continue;
            }
            ret.insert(
                row.0.clone(),
                TransitionalPackage {
                    from_name: row.0,
                    to_expr: e.to_string(),
                },
            );
        } else {
            log::debug!("no replacement for {}", row.0);
        }
    }
    Ok(ret)
}
