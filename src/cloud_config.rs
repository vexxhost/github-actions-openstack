use octocrab::models::actions::SelfHostedRunnerJitConfig;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Data {
    pub write_files: Vec<WriteFile>,
    pub runcmd: Vec<String>,
}

impl From<&SelfHostedRunnerJitConfig> for Data {
    fn from(config: &SelfHostedRunnerJitConfig) -> Self {
        let template = include_str!("../scripts/start.sh");
        let content = template.replace("___JIT_CONFIG___", &config.encoded_jit_config);

        Self {
            write_files: vec![WriteFile {
                path: "/start.sh".into(),
                permissions: "0755".into(),
                content,
            }],
            runcmd: vec!["/start.sh".into()],
        }
    }
}

impl Data {
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
