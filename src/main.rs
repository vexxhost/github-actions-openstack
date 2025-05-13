mod cloud_config;
mod config;

use crate::config::Config;
use anyhow::Result;
use axum::{Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};
use axum_github_hooks::GithubWebhook;
use chrono::DateTime;
use config::Pool;
use futures::{StreamExt, stream};
use octocrab::models::actions::SelfHostedRunner;
use openstack_types::compute::v2::server::response::list_detailed::ServerResponse;
use std::{collections::HashMap, sync::Arc};
use tracing::{Instrument, instrument};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone)]
struct AppState {
    config: Config,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer().with_target(true).with_level(true))
        .with(EnvFilter::from_default_env())
        .init();

    let config = Config::load().await?;
    let app_state = AppState {
        config: config.clone(),
    };

    let app = Router::new()
        .route("/webhook", post(webhook))
        .with_state(app_state.clone());

    tokio::spawn(async move {
        loop {
            // Handle errors outside the maintenance cycle span
            if let Err(e) = maintain_min_ready(config.clone()).await {
                tracing::error!(error = %e, "failed to maintain minimum ready nodes");
            }

            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }
    });

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}

async fn webhook(
    State(state): State<AppState>,
    GithubWebhook(hook): GithubWebhook,
) -> impl IntoResponse {
    println!("Received webhook: {:?}", hook);
    println!("Using OpenStack auth URL: {:?}", state.config);

    StatusCode::OK
}

#[instrument(skip(config))]
async fn maintain_min_ready(mut config: Config) -> Result<()> {
    for pool in config.pools.iter() {
        maintain_min_ready_for_pool(config.clone(), pool).await?;
    }

    let instances = config.openstack.list_nodes().await?;
    let runners = config.github.get_runners(None).await?;

    let runner_map: HashMap<String, &SelfHostedRunner> =
        runners.iter().map(|r| (r.name.clone(), r)).collect();

    for instance in &instances {
        if should_delete_instance(instance, runner_map.get(&instance.name))? {
            if let Err(e) = config.openstack.delete_node(instance).await {
                tracing::error!(error = %e, "failed to delete instance");
            } else {
                tracing::info!("successfully deleted instance");
            }
        }
    }

    let active_instances: HashMap<String, ServerResponse> = instances
        .iter()
        .filter(|n| n.status.as_deref() == Some("ACTIVE") || n.status.as_deref() == Some("BUILD"))
        .map(|n| (n.name.clone(), n.clone()))
        .collect();

    for runner in &runners {
        if should_delete_runner(runner, active_instances.get(&runner.name))? {
            if let Err(e) = config.github.delete_runner(runner).await {
                tracing::error!(error = %e, "failed to delete runner");
            } else {
                tracing::info!("successfully deleted runner");
            }
        }
    }

    tracing::info!("completed maintenance cycle");
    Ok(())
}

#[instrument(skip(instance, runner), fields(
    name = %instance.name,
    status = ?instance.status,
    created_at = ?instance.created,
    runner_status = %runner.map_or("none", |r| r.status.as_str()),
    busy = runner.map_or(false, |r| r.busy),
))]
fn should_delete_instance(
    instance: &ServerResponse,
    runner: Option<&&SelfHostedRunner>,
) -> Result<bool> {
    if let Some(created_at) = instance.created.clone() {
        let created_at = match DateTime::parse_from_rfc3339(&created_at) {
            Ok(dt) => dt,
            Err(e) => {
                tracing::warn!(error = %e, "invalid date format for node creation time");
                return Err(e.into());
            }
        };

        let node_age = chrono::Utc::now() - created_at.with_timezone(&chrono::Utc);
        tracing::debug!(age_minutes = %node_age.num_minutes(), "calculated node age");

        if node_age < chrono::Duration::minutes(5) {
            tracing::info!("instance is less than 5 minutes old, skipping checks");
            return Ok(false);
        }
    }

    Ok(match runner {
        Some(runner) if runner.busy => {
            tracing::info!("instance is busy, keeping");
            false
        }
        Some(runner) if runner.status.as_str() == "online" => {
            tracing::info!("instance is online, keeping");
            false
        }
        _ => {
            tracing::info!("deleting unused instance");
            true
        }
    })
}

#[instrument(skip(runner, instance), fields(
    name = %runner.name,
    status = %runner.status,
    busy = runner.busy,
))]
fn should_delete_runner(
    runner: &SelfHostedRunner,
    instance: Option<&ServerResponse>,
) -> Result<bool> {
    if let Some(instance) = instance {
        if instance.status.as_deref() == Some("ACTIVE")
            || instance.status.as_deref() == Some("BUILD")
        {
            tracing::info!("runner has active instance, keeping");
            return Ok(false);
        }
    }

    Ok(true)
}

#[instrument(skip(config, pool), fields(
    pool_labels = ?pool.runner.labels,
    min_ready = pool.min_ready,
    runner_group_id = pool.runner.group_id
))]
async fn maintain_min_ready_for_pool(config: Config, pool: &Pool) -> Result<()> {
    let runners = config
        .github
        .get_runners(Some(&pool.runner.labels[0]))
        .await?;

    let idle_runners_count = runners.iter().filter(|runner| !runner.busy).count();
    tracing::info!(
        total_runners = runners.len(),
        idle_runners = idle_runners_count,
        busy_runners = runners.len() - idle_runners_count,
        "completed runner inventory"
    );

    let nodes_to_create = if pool.min_ready > idle_runners_count as u32 {
        pool.min_ready - idle_runners_count as u32
    } else {
        0
    };

    tracing::info!(
        required = pool.min_ready,
        available = idle_runners_count,
        deficit = nodes_to_create,
        "calculated scaling requirements"
    );

    if nodes_to_create > 0 {
        tracing::info!(
            nodes_to_create = nodes_to_create,
            "initiating pool scaling operation"
        );

        let pool = Arc::new(pool.clone());

        // Create a stream of node creation tasks
        let results = stream::iter((0..nodes_to_create).map(|i| {
            let pool = Arc::clone(&pool);
            let node_index = i + 1;

            {
                let config = config.clone();

                async move {
                    add_runner(config, &pool)
                        .await
                        .map(|_| {
                            tracing::info!(node_index, "successfully created node");
                            (true, node_index)
                        })
                        .unwrap_or_else(|e| {
                            tracing::error!(error = %e, node_index, "failed to create node");
                            (false, node_index)
                        })
                }
            }
        }))
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;

        // Summarize results
        let successful = results.iter().filter(|(success, _)| *success).count();
        let failed = results.len() - successful;

        tracing::info!(
            requested = nodes_to_create,
            successful = successful,
            failed = failed,
            "completed scaling operation"
        );
    } else {
        tracing::debug!("no scaling needed, pool has sufficient idle runners");
    }

    tracing::info!("completed pool maintenance");
    Ok(())
}

#[instrument(skip(config, pool), fields(
    pool_labels = ?pool.runner.labels,
    runner_group_id = pool.runner.group_id
))]
async fn add_runner(mut config: Config, pool: &Pool) -> Result<()> {
    let jitconfig = config.github.generate_jitconfig(&pool.runner).await?;

    if let Err(e) = config.openstack.spawn_node(pool, &jitconfig).await {
        tracing::error!(error = %e, "failed to spawn node");

        if let Err(cleanup_error) = async {
            tracing::info!("cleaning up runner token due to instance creation failure");
            config.github.delete_runner(&jitconfig.runner).await
        }
        .instrument(tracing::info_span!(
            "cleanup_after_failure",
            runner_name = %jitconfig.runner.name
        ))
        .await
        {
            tracing::warn!(
                error = %cleanup_error,
                "failed to clean up runner token after instance creation failure"
            );
        } else {
            tracing::info!("successfully cleaned up runner token");
        }

        Err(e.into())
    } else {
        tracing::info!("successfully spawned node");

        Ok(())
    }
}
