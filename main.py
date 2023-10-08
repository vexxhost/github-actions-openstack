#!/usr/bin/env python3

from concurrent.futures import ThreadPoolExecutor
from datetime import datetime
import random
import string
import os
import yaml

from dotenv import load_dotenv
from flask import Flask
from flask_apscheduler import APScheduler
from github_webhook import Webhook

import openstack
import requests

load_dotenv()
CLOUD = openstack.connect()

app = Flask(__name__)
webhook = Webhook(app, endpoint="/webhook")

scheduler = APScheduler()
scheduler.init_app(app)
scheduler.start()


@webhook.hook(event_type="workflow_job")
def on_workflow_job(data):
    org = data["organization"]["login"]
    if org != os.environ["ORG"]:
        return

    labels = data["workflow_job"]["labels"]
    if os.environ["RUNNER_LABEL"] not in labels:
        return

    if data["action"] == "queued":
        scale_up()

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
    runners = get_runners_by_label(os.environ["ORG"], os.environ["RUNNER_LABEL"])
    idle_runners = [
        runner
        for runner in runners
        # NOTE(mnaser): Once scale_up ensures that the runner is ready, we
        #               should be able to add check here for online runners
        if runner["busy"] is False
    ]

    app.logger.info(
        "Found %s runners, %s idle runners, min_ready=%s",
        len(runners),
        len(idle_runners),
        os.environ["MIN_READY"],
    )

    nodes_to_create = int(os.environ["MIN_READY"]) - len(idle_runners)
    if nodes_to_create > 0:
        app.logger.info("Scaling up %s nodes", nodes_to_create)

        with ThreadPoolExecutor(max_workers=4) as executor:
            for _ in range(nodes_to_create):
                executor.submit(scale_up)
            executor.shutdown(wait=True)

    servers = [s for s in CLOUD.compute.servers() if s.name.startswith("gha-")]

    runners = get_runners_by_label(os.environ["ORG"], os.environ["RUNNER_LABEL"])
    runner_names = [runner["name"] for runner in runners]
    for server in servers:
        if server.name in runner_names:
            continue

        app.logger.info("Deleting server %s", server.name)
        CLOUD.compute.delete_server(server)


def scale_up():
    app.logger.info("Scaling up")

    name = generate_name()
    jitconfig = generate_jitconfig_for_organization(
        os.environ["ORG"],
        name,
        int(os.environ["RUNNER_GROUP_ID"]),
        [os.environ["RUNNER_LABEL"]],
    )
    cloud_init = generate_cloud_config_with_jitconfig(jitconfig)

    server = CLOUD.create_server(
        name=name,
        image=os.environ["IMAGE"],
        flavor=os.environ["FLAVOR"],
        network=os.environ["NETWORK"],
        userdata=cloud_init,
        wait=True,
        key_name=os.environ.get("KEY_NAME"),
    )

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
            "Authorization": "Bearer " + os.environ["GITHUB_TOKEN"],
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
            "Authorization": "Bearer " + os.environ["GITHUB_TOKEN"],
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
