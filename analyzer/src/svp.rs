use std::collections::HashMap;

#[derive(Debug, serde::Serialize)]
pub struct Failure {
    pub result_code: String,
    pub versions: HashMap<String, String>,
    pub description: String,
    pub transient: Option<bool>,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}: {}", self.result_code, self.description)
    }
}

impl std::error::Error for Failure {}

impl std::fmt::Display for Success {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Success")
    }
}

#[derive(Debug, serde::Serialize)]
pub struct ChangelogBehaviour {
    pub update: bool,
    pub explanation: String,
}

#[derive(Debug, serde::Serialize)]
pub struct DebianContext {
    pub changelog: Option<ChangelogBehaviour>,
}

#[derive(Debug, serde::Serialize)]
pub struct Success {
    pub versions: HashMap<String, String>,
    pub value: Option<i32>,
    pub context: Option<serde_json::Value>,
    pub debian: Option<DebianContext>,
}

pub fn write_svp_success(data: &Success) -> std::io::Result<()> {
    if enabled() {
        let f = std::fs::File::create(std::env::var("SVP_RESULT").unwrap()).unwrap();

        Ok(serde_json::to_writer(f, data)?)
    } else {
        Ok(())
    }
}

pub fn write_svp_failure(data: &Failure) -> std::io::Result<()> {
    if enabled() {
        let f = std::fs::File::create(std::env::var("SVP_RESULT").unwrap()).unwrap();

        Ok(serde_json::to_writer(f, data)?)
    } else {
        Ok(())
    }
}

pub fn report_success<T>(versions: HashMap<String, String>, value: Option<i32>, context: Option<T>)
where
    T: serde::Serialize,
{
    write_svp_success(&Success {
        versions,
        value,
        context: context.map(|x| serde_json::to_value(x).unwrap()),
        debian: None,
    })
    .unwrap();
}

pub fn report_success_debian<T>(
    versions: HashMap<String, String>,
    value: Option<i32>,
    context: Option<T>,
    changelog: Option<(bool, String)>,
) where
    T: serde::Serialize,
{
    write_svp_success(&Success {
        versions,
        value,
        context: context.map(|x| serde_json::to_value(x).unwrap()),
        debian: Some(DebianContext {
            changelog: changelog.map(|cl| ChangelogBehaviour {
                update: cl.0,
                explanation: cl.1,
            }),
        }),
    })
    .unwrap();
}

pub fn report_nothing_to_do(versions: HashMap<String, String>, description: Option<&str>) -> ! {
    let description = description.unwrap_or("Nothing to do");
    write_svp_failure(&Failure {
        result_code: "nothing-to-do".to_string(),
        versions,
        description: description.to_string(),
        transient: None,
    })
    .unwrap();
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
    write_svp_failure(&Failure {
        result_code: code.to_string(),
        versions,
        description: description.to_string(),
        transient,
    })
    .unwrap();
    log::error!("{}", description);
    if let Some(hint) = hint {
        log::info!("{}", hint);
    }
    std::process::exit(1);
}

pub fn load_resume() -> Option<serde_json::Value> {
    if enabled() {
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
