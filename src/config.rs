use crate::cloud_config;
use base64::prelude::*;
use chrono::TimeDelta;
use octocrab::{
    Octocrab,
    models::{
        RunnerGroupId,
        actions::{SelfHostedRunner, SelfHostedRunnerJitConfig},
    },
};
use openstack_sdk::{
    AsyncOpenStack,
    api::{
        self, QueryAsync,
        compute::v2::server::{create_20, delete, list_detailed},
    },
    auth::AuthState,
    config::ConfigFile,
    types::ServiceType,
};
use openstack_types::compute::v2::server::response::{
    create::ServerResponse as CreateServerResponse,
    list_detailed::ServerResponse as ListServerResponse,
};
use rand::Rng;
use serde::Deserialize;
use std::borrow::Cow;
use thiserror::Error;
use tracing::instrument;

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    pub github: GitHub,
    pub openstack: OpenStack,
    pub pools: Vec<Pool>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GitHub {
    org: String,
    token: String,
}

impl GitHub {
    fn client(&self) -> octocrab::Result<Octocrab> {
        octocrab::OctocrabBuilder::default()
            .personal_token(self.token.clone())
            .build()
    }

    #[instrument(skip(self), fields(org = %self.org))]
    pub async fn get_runners(
        &self,
        filter_label: Option<&String>,
    ) -> octocrab::Result<Vec<SelfHostedRunner>> {
        let octocrab = self.client()?;
        let mut runners = vec![];

        let mut page = octocrab
            .actions()
            .list_org_self_hosted_runners(&self.org)
            .send()
            .await?;

        loop {
            for runner in &page.items {
                if !runner.name.starts_with("gha-") {
                    continue;
                }

                if filter_label.is_none()
                    || (filter_label.as_ref().is_some_and(|label| {
                        runner
                            .labels
                            .iter()
                            .any(|runner_label| runner_label.name == **label)
                    }))
                {
                    runners.push(runner.clone());
                }
            }

            page = match octocrab.get_page(&page.next).await? {
                Some(next_page) => next_page,
                None => break,
            };
        }

        Ok(runners)
    }

    #[instrument(
        skip(self, runner),
        fields(org = %self.org, group_id = %runner.group_id, labels = ?runner.labels)
    )]
    pub async fn generate_jitconfig(
        &self,
        runner: &PoolRunner,
    ) -> octocrab::Result<SelfHostedRunnerJitConfig> {
        let octocrab = self.client()?;
        match octocrab
            .actions()
            .create_org_jit_runner_config(
                &self.org,
                runner.generate_name(),
                RunnerGroupId(runner.group_id),
                runner.labels.clone(),
            )
            .send()
            .await
        {
            Ok(config) => {
                tracing::info!(
                    runner_name = %config.runner.name,
                    "successfully generated runner jitconfig"
                );
                Ok(config)
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "failed to generate runner jitconfig"
                );
                Err(e)
            }
        }
    }

    #[instrument(
        skip(self, runner),
        fields(org = %self.org, runner_id = %runner.id, runner_name = %runner.name)
    )]
    pub async fn delete_runner(&self, runner: &SelfHostedRunner) -> octocrab::Result<()> {
        let octocrab = self.client()?;
        match octocrab
            .actions()
            .delete_org_runner(&self.org, runner.id)
            .await
        {
            Ok(_) => {
                tracing::info!("successfully deleted github runner");
                Ok(())
            }
            Err(e) => {
                tracing::error!(error = %e, "failed to delete github runner");
                Err(e)
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct OpenStack {
    cloud: String,

    #[serde(skip)]
    session: Option<AsyncOpenStack>,
}

#[derive(Debug, Error)]
pub enum OpenStackError {
    #[error("missing OpenStack session")]
    MissingSession,

    #[error("failed to build network request")]
    BuildNetworkRequest(#[from] create_20::NetworksBuilderError),

    #[error(transparent)]
    Serialization(#[from] serde_yaml::Error),

    #[error("failed to build server request")]
    BuildServerRequest(#[from] create_20::ServerBuilderError),

    #[error("failed to build request")]
    BuildRequest(#[from] create_20::RequestBuilderError),

    #[error("failed to build server list request")]
    BuildServerListRequest(#[from] list_detailed::RequestBuilderError),

    #[error("failed to build server deletion request")]
    BuildServerDeletionRequest(#[from] delete::RequestBuilderError),

    #[error(transparent)]
    Api(#[from] openstack_sdk::api::ApiError<openstack_sdk::RestError>),

    #[error(transparent)]
    OpenStack(#[from] openstack_sdk::OpenStackError),
}

impl OpenStack {
    #[instrument(
        skip(self),
        fields(cloud = %self.cloud)
    )]
    async fn session(&mut self) -> Result<&AsyncOpenStack, OpenStackError> {
        tracing::debug!("checking openstack session");
        let session = self
            .session
            .as_mut()
            .ok_or(OpenStackError::MissingSession)?;

        match session.get_auth_state(Some(TimeDelta::seconds(10))) {
            Some(AuthState::Expired) | Some(AuthState::AboutToExpire) => {
                session.authorize(None, false, true).await?;
                session
                    .discover_service_endpoint(&ServiceType::Compute)
                    .await?;
            }
            _ => {}
        }

        Ok(session)
    }

    #[instrument(
        skip(self),
        fields(cloud = %self.cloud)
    )]
    pub async fn list_nodes(&mut self) -> Result<Vec<ListServerResponse>, OpenStackError> {
        let session = self.session().await?;

        tracing::debug!("building server list request");
        let ep = list_detailed::Request::builder().build().map_err(|e| {
            tracing::error!(error = %e, "failed to build server list request");
            e
        })?;

        let data: Vec<ListServerResponse> = ep.query_async(session).await.map_err(|e| {
            tracing::error!(error = %e, "failed to query server list");
            e
        })?;

        Ok(data
            .iter()
            .filter(|s| s.name.starts_with("gha-"))
            .cloned()
            .collect())
    }

    #[instrument(
        skip(self, pool, jitconfig),
        fields(
            runner_name = %jitconfig.runner.name,
            image = %pool.instance.image,
            flavor = %pool.instance.flavor,
            network = %pool.instance.network
        )
    )]
    pub async fn spawn_node(
        &mut self,
        pool: &Pool,
        jitconfig: &SelfHostedRunnerJitConfig,
    ) -> Result<(), OpenStackError> {
        tracing::debug!("preparing cloud-init configuration");
        let cloud_init: cloud_config::Data = jitconfig.into();

        let session = self.session().await?;

        tracing::debug!("building server creation request");
        let user_data = cloud_init.to_user_data()?;
        let ep = match create_20::Request::builder()
            .server(
                create_20::ServerBuilder::default()
                    .name(&jitconfig.runner.name)
                    .image_ref(&pool.instance.image)
                    .flavor_ref(&pool.instance.flavor)
                    .networks(vec![
                        create_20::NetworksBuilder::default()
                            .uuid(&pool.instance.network)
                            .build()?,
                    ])
                    .key_name(&pool.instance.key_name)
                    .user_data(Some(Cow::Owned(BASE64_STANDARD.encode(user_data))))
                    .build()?,
            )
            .build()
        {
            Ok(ep) => ep,
            Err(e) => {
                tracing::error!(error = %e, "failed to build server request");
                return Err(e.into());
            }
        };

        let _data: CreateServerResponse = ep.query_async(session).await?;

        // NOTE(mnaser): We should ideally wait for the node to become ACTIVE
        //               before returning, but for now we just return the request
        //               and let the caller handle it.

        tracing::info!("successfully spawned node");

        Ok(())
    }

    #[instrument(
        skip(self, node),
        fields(node_id = %node.id)
    )]
    pub async fn delete_node(&mut self, node: &ListServerResponse) -> Result<(), OpenStackError> {
        let session = self.session().await?;

        tracing::debug!("building server deletion request");
        let ep = delete::Request::builder().id(&node.id).build()?;

        api::ignore(ep).query_async(session).await?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Pool {
    pub min_ready: u32,
    pub runner: PoolRunner,
    pub instance: Instance,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PoolRunner {
    pub group_id: u64,
    pub labels: Vec<String>,
}

impl PoolRunner {
    pub fn generate_name(&self) -> String {
        format!(
            "gha-{}",
            rand::rng()
                .sample_iter(rand::distr::Alphanumeric)
                .filter(|c| c.is_ascii_lowercase())
                .take(5)
                .map(char::from)
                .collect::<String>()
        )
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Instance {
    key_name: String,
    pub flavor: String,
    pub image: String,
    network: String,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to load config file")]
    ConfigFile(#[from] config::ConfigError),

    #[error("openstack profile not found: {0}")]
    OpenStackProfile(String),

    #[error(transparent)]
    OpenStackConfig(#[from] openstack_sdk::config::ConfigError),

    #[error(transparent)]
    OpenStack(#[from] openstack_sdk::OpenStackError),
}

impl Config {
    pub async fn load() -> Result<Self, ConfigError> {
        let settings = config::Config::builder()
            .add_source(config::File::with_name("config"))
            .build()?;

        let mut cfg = settings.try_deserialize::<Config>()?;

        let profile = match ConfigFile::new()?.get_cloud_config(&cfg.openstack.cloud)? {
            Some(profile) => profile,
            None => return Err(ConfigError::OpenStackProfile(cfg.openstack.cloud.clone())),
        };

        let mut session = AsyncOpenStack::new(&profile).await?;
        session
            .discover_service_endpoint(&ServiceType::Compute)
            .await?;
        cfg.openstack.session = Some(session);

        Ok(cfg)
    }
}
