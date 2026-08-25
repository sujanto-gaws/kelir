import { getOperational } from './client'

/**
 * How the backend this build is talking to is configured (`GET /deployment`).
 *
 * **Why an endpoint and not a `VITE_` flag.** A build-time flag would be
 * cheaper and would couple the frontend *image* to one backend's setting. The
 * image bakes only `VITE_KELIR_API_BASE_URL=/api/v1` — a relative path — so one
 * build serves every deployment today, and a build-time tenancy flag would end
 * that. Decision **D-18** takes the endpoint; #67 is where the question was
 * asked.
 *
 * Operational, so it lives at the root outside `/api/v1` and answers without
 * the response envelope — hence `getOperational`, the same route
 * `DashboardPage` takes to `/version`. The shape is declared here rather than
 * in `types/` for that reason: it is not part of the enveloped API surface the
 * other wire types describe.
 */
export interface Deployment {
  /**
   * Whether this deployment serves more than one tenant, and therefore whether
   * `POST /auth/login` requires `tenantCode`.
   */
  multiTenant: boolean
}

/**
 * Ask the backend which mode it is in.
 *
 * Unauthenticated: the login form needs the answer before there is anything to
 * authenticate with. Callers are expected to survive a rejection — see
 * `LoginPage`, which falls back to the single-tenant form and still recovers if
 * the guess was wrong.
 */
export function fetchDeployment(): Promise<Deployment> {
  return getOperational<Deployment>('/deployment')
}
