#!/usr/bin/env python3

import concurrent.futures
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime
import random
import string
import yaml

from flask import Flask
from flask_apscheduler import APScheduler
from github_webhook import Webhook

import openstack
import requests

with open("config.yml", "r", encoding="utf-8") as fd:
    CFG = yaml.safe_load(fd)
CLOUD = openstack.connect(cloud=CFG["openstack"]["cloud"])

app = Flask(__name__)
webhook = Webhook(app, endpoint="/webhook")

scheduler = APScheduler()
scheduler.init_app(app)
scheduler.start()


@webhook.hook(event_type="workflow_job")
def on_workflow_job(data):
    org = data["organization"]["login"]
    if org != CFG["github"]["org"]:
        return

    labels = data["workflow_job"]["labels"]

    if data["action"] == "queued":
        for pool in CFG["pools"]:
            if pool["runner"]["label"] in labels:
                scale_up(pool)
                return

    if data["action"] == "completed":
        runner_name = data["workflow_job"]["runner_name"]
        app.logger.info("Deleting runner %s", runner_name)
        CLOUD.compute.delete_server(runner_name)


@scheduler.task(
    "interval",
    id="maintain_min_ready",
    seconds=30,
    max_instances=1,
    next_run_time=datetime.now(),
)
def maintain_min_ready():
    for pool in CFG["pools"]:
        maintain_min_ready_for_pool(pool)

    servers = [s for s in CLOUD.compute.servers() if s.name.startswith("gha-")]
    runners = get_runners_for_organization(CFG["github"]["org"])
    runner_names = [runner["name"] for runner in runners]
    for server in servers:
        if server.name in runner_names:
            continue

        app.logger.info("Deleting server %s", server.name)
        CLOUD.compute.delete_server(server)


def maintain_min_ready_for_pool(pool: dict):
    runners = get_runners_by_label(CFG["github"]["org"], pool["runner"]["label"])
    idle_runners = [
        runner
        for runner in runners
        # NOTE(mnaser): Once scale_up ensures that the runner is ready, we
        #               should be able to add check here for online runners
        if runner["busy"] is False
    ]

    app.logger.info(
        "%s: Found %s runners, %s idle runners, min_ready=%s",
        pool["runner"]["label"],
        len(runners),
        len(idle_runners),
        pool["min_ready"],
    )

    nodes_to_create = pool["min_ready"] - len(idle_runners)
    if nodes_to_create > 0:
        app.logger.info("Scaling up %s nodes", nodes_to_create)

        with ThreadPoolExecutor(max_workers=4) as executor:
            for _ in range(nodes_to_create):
                future_to_scale_up = {
                    executor.submit(scale_up, pool): pool["instance"]["flavor"]
                    for _ in range(nodes_to_create)
                }

            for future in concurrent.futures.as_completed(future_to_scale_up):
                future.result()

            executor.shutdown(wait=True)


def scale_up(pool: dict):
    app.logger.info("Scaling up")

    name = generate_name()
    jitconfig = generate_jitconfig_for_organization(
        CFG["github"]["org"],
        name,
        pool["runner"]["group"],
        [pool["runner"]["label"]],
    )
    cloud_init = generate_cloud_config_with_jitconfig(jitconfig)

    server = CLOUD.create_server(
        name=name,
        image=pool["instance"]["image"],
        flavor=pool["instance"]["flavor"],
        network=pool["instance"]["network"],
        key_name=pool["instance"].get("key_name"),
        userdata=cloud_init,
        wait=True,
    )

    # TODO: If we fail here, we should delete the runner token

    app.logger.info("Created server %s", server.name)

    # NOTE(mnaser): We should ideally wait for the runner to be ready inside
    #               GHA, if not we drop out.


def generate_cloud_config_with_jitconfig(jitconfig: str):
    cloud_config = {
        "write_files": [],
        "runcmd": [
            "/start.sh",
        ],
    }

    with open("scripts/start.sh", "r", encoding="utf-8") as f:
        cloud_config["write_files"].append(
            {
                "path": "/start.sh",
                "content": f.read().replace("___JIT_CONFIG___", jitconfig),
                "permissions": "0755",
            }
        )

    return "#cloud-config\n" + yaml.dump(cloud_config)


def get_runners_for_organization(org: str):
    response = requests.get(
        "https://api.github.com/orgs/" + org + "/actions/runners",
        timeout=5,
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": "Bearer " + CFG["github"]["token"],
            "X-GitHub-Api-Version": "2022-11-28",
        },
    )
    response.raise_for_status()
    return response.json().get("runners", [])


def get_runners_by_label(org: str, label: str):
    runners = get_runners_for_organization(org)
    return [
        runner
        for runner in runners
        if label in [label["name"] for label in runner["labels"]]
    ]


def generate_jitconfig_for_organization(
    org: str, name: str, runner_group_id: int, labels: list[str]
):
    response = requests.post(
        "https://api.github.com/orgs/" + org + "/actions/runners/generate-jitconfig",
        timeout=5,
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": "Bearer " + CFG["github"]["token"],
            "X-GitHub-Api-Version": "2022-11-28",
        },
        json={
            "name": name,
            "runner_group_id": runner_group_id,
            "labels": labels,
        },
    )
    response.raise_for_status()
    return response.json().get("encoded_jit_config")


def generate_name():
    letters = string.ascii_lowercase
    suffix = "".join(random.choice(letters) for i in range(5))
    return "gha-" + suffix


if __name__ == "__main__":
    app.run()
