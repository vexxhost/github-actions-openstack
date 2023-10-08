#!/bin/bash -xe

RUNNER_USER=${RUNNER_USER:-runner}
RUNNER_GROUP=${RUNNER_GROUP:-runner}
RUNNER_VERSION=${RUNNER_VERSION:-2.309.0}
RUNNER_CHECKSUM=${RUNNER_CHECKSUM:-2974243bab2a282349ac833475d241d5273605d3628f0685bd07fb5530f9bb1a}
RUNNER_JITCONFIG=___JIT_CONFIG___

# Create "runner" group if it doesn't exist
if ! getent group ${RUNNER_GROUP}; then
    groupadd -r ${RUNNER_GROUP}
fi

# Create "runner" user if it doesn't exist
if ! getent passwd ${RUNNER_USER}; then
    useradd -r -g ${RUNNER_USER} -d /runner -c "GitHub Actions Runner" runner
fi

# Add "runner" user to the sudoers
echo "runner ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers

# Create folders for runner
mkdir -p /runner
cd /runner

# Download the runner package
curl -o actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz -L https://github.com/actions/runner/releases/download/v${RUNNER_VERSION}/actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz
echo "${RUNNER_CHECKSUM}  actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz" | shasum -a 256 -c
tar xzf ./actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz
chown -R ${RUNNER_USER}:${RUNNER_GROUP} /runner

# Start the runner
systemd-run --uid=runner --gid=runner ./run.sh --jitconfig ${RUNNER_JITCONFIG}
