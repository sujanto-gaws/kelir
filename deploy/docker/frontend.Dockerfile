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
#   docker build -f deploy/docker/frontend.Dockerfile -t kelir-frontend:0.1.0 kelir-frontend

FROM node:24-alpine AS builder

WORKDIR /build

COPY package.json package-lock.json ./
RUN npm ci

COPY . .

# Same origin as the site itself; Caddy routes it onward.
ENV VITE_KELIR_API_BASE_URL=/api/v1
RUN npm run build

FROM caddy:2-alpine AS runtime

# Only the built site lives in the image. The Caddyfile is supplied by the
# environment's compose file, so the same image serves staging and production
# without a rebuild.
COPY --from=builder /build/dist /srv

EXPOSE 80 443
