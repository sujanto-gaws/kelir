# Deployment Files

Compose files, container images and deployment scripts. The guide that explains
how to use them is [docs/operations/01. Installation and Deployment.md](../docs/operations/01.%20Installation%20and%20Deployment.md).

```text
deploy/
├── docker/
│   ├── docker-compose.yml       development stack: source mounted, hot reload
│   ├── backend.Dockerfile       release image, multi-stage
│   └── frontend.Dockerfile      release image: built bundle plus Caddy
├── env/
│   └── .env.example             development configuration template
└── staging/
    ├── provision-ubuntu-24.sh   one-time host setup (Docker, PostgreSQL, firewall, backups)
    ├── deploy.sh                deploy a version to a hostname, over TLS
    ├── deploy-local.sh          deploy to http://<host-ip>:<port> for testing
    ├── docker-compose.staging.yml
    ├── Caddyfile                single-origin routing; TLS when the address is a hostname
    └── .env.staging.example     deployment secrets template
```

Quick reference:

| Task | Command |
|---|---|
| Develop | `docker compose -f deploy/docker/docker-compose.yml up` |
| **Release check** — bring the stack up from release images and smoke-test it | `cd deploy/staging && ./deploy-local.sh 0.1.0` |
| Provision a staging host — **unused, no host exists** ([why](staging/README.md)) | `sudo ./provision-ubuntu-24.sh` |
| Deploy to staging — **unused, no host exists** ([why](staging/README.md)) | `./deploy.sh 0.1.0` |
