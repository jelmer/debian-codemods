use std::collections::HashMap;

#[derive(Debug, serde::Serialize)]
struct Failure {
    result_code: String,
    versions: HashMap<String, String>,
    description: String,
    transient: Option<bool>,
}

#[derive(Debug, serde::Serialize)]
struct ChangelogBehaviour {
    update: bool,
    explanation: String,
}

#[derive(Debug, serde::Serialize)]
struct DebianContext {
    changelog: Option<ChangelogBehaviour>,
}

#[derive(Debug, serde::Serialize)]
struct Success {
    versions: HashMap<String, String>,
    value: Option<i32>,
    context: Option<serde_json::Value>,
    debian: Option<DebianContext>,
}

pub fn report_success<T>(
    versions: HashMap<String, String>,
    value: Option<i32>,
    context: Option<T>
) where T: Into<serde_json::Value> {
    if std::env::var("SVP_API").ok().as_deref() == Some("1") {
        let f = std::fs::File::create(std::env::var("SVP_RESULT").unwrap()).unwrap();

        serde_json::to_writer(
            f,
            &Success {
                versions,
                value,
                context: context.map(|x| x.into()),
                debian: None,
            },
        )
        .unwrap();
    }
}

pub fn report_success_debian<T>(
    versions: HashMap<String, String>,
    value: Option<i32>,
    context: Option<T>,
    changelog: Option<(bool, String)>,
) where T: Into<serde_json::Value>{
    if std::env::var("SVP_API").ok().as_deref() == Some("1") {
        let f = std::fs::File::create(std::env::var("SVP_RESULT").unwrap()).unwrap();

        serde_json::to_writer(
            f,
            &Success {
                versions,
                value,
                context: context.map(|x| x.into()),
                debian: Some(DebianContext {
                    changelog: changelog.map(|cl| ChangelogBehaviour {
                        update: cl.0,
                        explanation: cl.1,
                    }),
                }),
            },
        )
        .unwrap();
    }
}

pub fn report_nothing_to_do(versions: HashMap<String, String>, description: Option<&str>) -> ! {
    let description = description.unwrap_or("Nothing to do");
    if std::env::var("SVP_API").ok().as_deref() == Some("1") {
        let f = std::fs::File::create(std::env::var("SVP_RESULT").unwrap()).unwrap();

        serde_json::to_writer(
            f,
            &Failure {
                result_code: "nothing-to-do".to_string(),
                versions,
                description: description.to_string(),
                transient: None,
            },
        )
        .unwrap();
    }
    log::error!("{}", description);
    std::process::exit(0);
}

pub fn report_fatal(
    versions: HashMap<String, String>,
    code: &str,
    description: &str,
    hint: Option<&str>,
    transient: Option<bool>,
) -> ! {
    if std::env::var("SVP_API").ok().as_deref() == Some("1") {
        let f = std::fs::File::create(std::env::var("SVP_RESULT").unwrap()).unwrap();

        serde_json::to_writer(
            f,
            &Failure {
                result_code: code.to_string(),
                versions,
                description: description.to_string(),
                transient,
            },
        )
        .unwrap();
    }
    log::error!("{}", description);
    if let Some(hint) = hint {
        log::info!("{}", hint);
    }
    std::process::exit(1);
}

pub fn load_resume() -> Option<serde_json::Value> {
    if std::env::var("SVP_API").ok().as_deref() == Some("1") {
        if let Ok(resume_path) = std::env::var("SVP_RESUME") {
            let f = std::fs::File::open(resume_path).unwrap();
            let resume: serde_json::Value = serde_json::from_reader(f).unwrap();
            Some(resume)
        } else {
            None
        }
    } else {
        None
    }
}

pub fn enabled() -> bool {
    std::env::var("SVP_API").ok().as_deref() == Some("1")
}
