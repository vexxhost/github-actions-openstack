#!/bin/bash -xe

RUNNER_USER=___RUNNER_USER___
RUNNER_GROUP=___RUNNER_GROUP___
RUNNER_VERSION=${RUNNER_VERSION:-2.327.0}
RUNNER_CHECKSUM=${RUNNER_CHECKSUM:-697deac53b39b72396c6fe3fe3b10bdc05cf59c12e82295a2e6decc53ca7d3e4}
RUNNER_JITCONFIG=___JIT_CONFIG___

# Download the runner package
mkdir -p /opt/github/actions-runner/${RUNNER_VERSION}
cd /opt/github/actions-runner/${RUNNER_VERSION}
curl -o actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz -L https://github.com/actions/runner/releases/download/v${RUNNER_VERSION}/actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz
echo "${RUNNER_CHECKSUM}  actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz" | shasum -a 256 -c
tar xzf ./actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz
chown -R ${RUNNER_USER}:${RUNNER_GROUP} /opt/github/actions-runner

# Start the runner
su - ${RUNNER_USER} -c "/opt/github/actions-runner/${RUNNER_VERSION}/run.sh --jitconfig ${RUNNER_JITCONFIG}" &
