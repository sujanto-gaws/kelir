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
    ├── kelir-backup.sh          the encrypted backup pair: database and object store
    ├── kelir-restore-objects.sh writes a store archive back into an object store
    ├── encryption-at-rest.probe.sh  reads the store and the archives raw (#778)
    ├── deploy.sh                deploy a version to a hostname, over TLS
    ├── deploy-local.sh          deploy to http://<host-ip>:<port> for testing
    ├── docker-compose.staging.yml
    ├── docker-compose.e2e.yml   overlay for the browser flows only: one system a test call can reach
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

**A new writer of objects asks for SSE-S3, and a new archive or export is encrypted to `KELIR_BACKUP_AGE_RECIPIENTS` before it touches a disk** ([ADR-0049](../docs/architectures/adr/0049.%20The%20Object%20Store%20Encrypts%20Attachments%20Under%20Its%20Own%20Key,%20and%20a%20Backup%20Is%20Encrypted%20to%20a%20Key%20the%20Host%20Does%20Not%20Hold.md) §5; the Installation guide §8).
