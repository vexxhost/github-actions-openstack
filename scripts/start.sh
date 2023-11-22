#!/bin/bash -xe

RUNNER_USER=${RUNNER_USER:-ubuntu}
RUNNER_GROUP=${RUNNER_GROUP:-ubuntu}
RUNNER_VERSION=${RUNNER_VERSION:-2.311.0}
RUNNER_CHECKSUM=${RUNNER_CHECKSUM:-29fc8cf2dab4c195bb147384e7e2c94cfd4d4022c793b346a6175435265aa278}
RUNNER_JITCONFIG=___JIT_CONFIG___

# Download the runner package
mkdir -p /opt/github/actions-runner/${RUNNER_VERSION}
cd /opt/github/actions-runner/${RUNNER_VERSION}
curl -o actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz -L https://github.com/actions/runner/releases/download/v${RUNNER_VERSION}/actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz
echo "${RUNNER_CHECKSUM}  actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz" | shasum -a 256 -c
tar xzf ./actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz
chown -R ${RUNNER_USER}:${RUNNER_GROUP} /opt/github/actions-runner

# Add the runner user to the docker group
usermod -aG docker ${RUNNER_USER}

# Start the runner
su - ubuntu -c "/opt/github/actions-runner/${RUNNER_VERSION}/run.sh --jitconfig ${RUNNER_JITCONFIG}" &
