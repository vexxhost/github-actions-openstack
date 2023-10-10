#!/bin/bash -xe

RUNNER_USER=${RUNNER_USER:-ubuntu}
RUNNER_GROUP=${RUNNER_GROUP:-ubuntu}
RUNNER_VERSION=${RUNNER_VERSION:-2.309.0}
RUNNER_CHECKSUM=${RUNNER_CHECKSUM:-2974243bab2a282349ac833475d241d5273605d3628f0685bd07fb5530f9bb1a}
RUNNER_JITCONFIG=___JIT_CONFIG___

# Download the runner package
mkdir -p /opt/github/actions-runner/${RUNNER_VERSION}
cd /opt/github/actions-runner/${RUNNER_VERSION}
curl -o actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz -L https://github.com/actions/runner/releases/download/v${RUNNER_VERSION}/actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz
echo "${RUNNER_CHECKSUM}  actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz" | shasum -a 256 -c
tar xzf ./actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz
chown -R ${RUNNER_USER}:${RUNNER_GROUP} /opt/github/actions-runner

# Start the runner
sudo -u ${RUNNER_USER} -s -- nohup ./run.sh --jitconfig ${RUNNER_JITCONFIG} &
