# Frontend release image — the built site plus the Caddy server that fronts the
# whole deployment.
#
# Caddy serves the static bundle at `/` and reverse-proxies the API paths to the
# backend, so the browser sees a single origin. That removes CORS from deployed
# environments entirely (it stays only for the local Vite dev server) and keeps
# Phase 2's session cookies same-site.
#
# The API base URL is baked in at build time because Vite inlines `import.meta.env`
# — it cannot be changed by an environment variable at run time. It is a relative
# path precisely so the same image works on any hostname.
#
# Build with the commit so the bundle can say which release it is (#362,
# release process §4 step 7):
#   docker build -f deploy/docker/frontend.Dockerfile #     --build-arg KELIR_BUILD_SHA=$(git rev-parse --short HEAD) #     -t kelir-frontend:0.1.0 kelir-frontend

# **Both bases are pinned to a release, not a line** (#590, D-62). The pin
# test reads every `FROM` here and refuses `node:24-alpine` and
# `caddy:2-alpine`, which follow every Node 24 and every Caddy 2 release. Each
# pin is the release its line resolved to on 2026-09-30, checked by index
# digest, so pinning froze the image rather than moving it. Caddy is a Docker
# Official Image, accepted on Docker Hub by #546's dispositions.
FROM node:24.21.0-alpine AS builder

WORKDIR /build

COPY package.json package-lock.json ./
RUN npm ci

COPY . .

# Same origin as the site itself; Caddy routes it onward.
ENV VITE_KELIR_API_BASE_URL=/api/v1

# **The commit reaches the bundle, and before this it reached nothing** (#362).
# `kelir-frontend:0.6.0`'s served bundle was byte-identical to `0.6.0-rc`'s
# because no build input carried the release, so Docker gave the new image the
# rc's creation timestamp and no image could be told from another. `vite.config`
# emits it to `version.json`; unset here it falls back to `git rev-parse` and
# then to `unknown`, which is `kelir-backend/build.rs`'s order exactly.
ARG KELIR_BUILD_SHA=
ENV KELIR_BUILD_SHA=${KELIR_BUILD_SHA}

RUN npm run build

FROM caddy:2.11.4-alpine AS runtime

# Only the built site lives in the image. The Caddyfile is supplied by the
# environment's compose file, so the same image serves staging and production
# without a rebuild.
#
# `/srv/version.json` travels with it: Caddy's `try_files {path} /index.html`
# serves a real file before falling back to the SPA, and `/version` is proxied
# to the backend, so the two identities sit side by side without colliding.
COPY --from=builder /build/dist /srv

EXPOSE 80 443
