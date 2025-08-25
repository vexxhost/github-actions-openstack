#!/bin/bash -xe

RUNNER_USER=___RUNNER_USER___
RUNNER_GROUP=___RUNNER_GROUP___
RUNNER_VERSION=${RUNNER_VERSION:-2.327.0}
RUNNER_JITCONFIG=___JIT_CONFIG___

if [ -f /etc/debian_version ]; then
    export DEBIAN_FRONTEND=noninteractive
    apt-get update
    apt-get install -y 'libicu[0-9][0-9]'
elif [ -f /etc/redhat-release ] || [ -f /etc/rocky-release ] || [ -f /etc/centos-release ]; then
    yum install -y libicu || dnf install -y libicu
fi

# Download the runner package
mkdir -p /opt/github/actions-runner/${RUNNER_VERSION}
cd /opt/github/actions-runner/${RUNNER_VERSION}
curl -o actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz -L https://github.com/actions/runner/releases/download/v${RUNNER_VERSION}/actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz
tar xzf ./actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz
chown -R ${RUNNER_USER}:${RUNNER_GROUP} /opt/github/actions-runner

# Start the runner
su - ${RUNNER_USER} -c "/opt/github/actions-runner/${RUNNER_VERSION}/run.sh --jitconfig ${RUNNER_JITCONFIG}" &
