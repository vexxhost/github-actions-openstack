# Auto-scaling self-hosted GitHub Actions runners on OpenStack

This repository contains a project which allows you to deploy a fleet of
self-hosted GitHub Actions runners on OpenStack.  This project was built,
tested and developed against the [VEXXHOST](https://vexxhost.com) public cloud
which is powered by [Atmosphere](https://vexxhost.com/private-cloud/atmosphere-openstack-depoyment/).

If you are using Atmosphere, you should be able to use this project out of the
box, but if you are using a different OpenStack provider, you may need to
modify the `cloud-init` script to work with your provider.

## Usage

You can use this project in three different ways in order to balance cost and
job start time.  In all these methods, the VMs are automatically cleaned up
when they are no longer needed.

### Runner pool

This method is the simplest and only requires you to have a GitHub token and
does not require any webhook configuration.  It will simply poll for the number
of idle runners and launch new VMs if the number of idle runners is below a
configured threshold.

```bash
MIN_READY=5
```

The configuration above will ensure that there are always at least 5 idle
runners available to run jobs.  Once a VM finishes running a job, it will
automatically be cleaned up (at maximum 30 seconds after the job finishes).

### Runner pool + web hooks

This method is similar to the previous one, but it also configures a webhook
which will be called by GitHub whenever a job is queued or completed.  This
allows the runner pool to spin up new VMs on-demand when jobs are queued and
then clean them up when they are no longer needed.

This method helps launch VMs as soon as jobs are queued, so it may help with
large bursts of jobs, because in the previous method, only `MIN_READY` VMs
are launched at a time, so it may take longer to start jobs if there are
many jobs queued at the same time.

```bash
MIN_READY=5
```

You will need to configure a webhook in GitHub to point to the webhook URL
which will be pointing to the server that is running this project.  The URL
will be something like `https://example.com/webhook`.

### Web hooks only

This method is the most cost effective but it would take longer to start jobs
because it does not keep any VMs running at all times.  Instead, it will
configure a webhook which will be called by GitHub whenever a job is queued or
completed.  This allows the runner pool to spin up new VMs on-demand when jobs
are queued and then clean them up when they are no longer needed.

```bash
MIN_READY=0
```

You will need to configure a webhook in GitHub to point to the webhook URL
which will be pointing to the server that is running this project.  The URL
will be something like `https://example.com/webhook`.

## Configuration

You can reference the `.env.example` file for a list of all the configuration
options.  You can either copy the `.env.example` file to `.env` and edit it
directly, or you can set the environment variables directly.

## Deployment

For simplicity, this project provides a `docker-compose.yml` file which can be
used to deploy the project.  You can also deploy it manually if you prefer.

### Docker Compose

```bash
git clone https://github.com/vexxhost/github-actions-openstack.git
cd github-actions-openstack
cp .env.example .env
# Edit .env to configure the project
docker-compose up -d
```
