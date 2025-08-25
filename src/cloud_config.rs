use crate::config::Pool;
use octocrab::models::actions::SelfHostedRunnerJitConfig;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Data {
    pub write_files: Vec<WriteFile>,
    pub runcmd: Vec<String>,
}

impl Data {
    pub fn from_jitconfig(config: &SelfHostedRunnerJitConfig, pool: &Pool) -> Self {
        let template = include_str!("../scripts/start.sh");
        let content = template
            .replace("___JIT_CONFIG___", &config.encoded_jit_config)
            .replace("___RUNNER_USER___", &pool.instance.runner_user)
            .replace("___RUNNER_GROUP___", &pool.instance.runner_group);

        Self {
            write_files: vec![WriteFile {
                path: "/start.sh".into(),
                permissions: "0755".into(),
                content,
            }],
            runcmd: vec!["/start.sh".into()],
        }
    }

    pub fn to_user_data(&self) -> serde_yaml::Result<String> {
        Ok(format!("#cloud-config\n{}", serde_yaml::to_string(self)?))
    }
}

#[derive(Debug, Serialize)]
pub struct WriteFile {
    pub path: String,
    pub content: String,
    pub permissions: String,
}
