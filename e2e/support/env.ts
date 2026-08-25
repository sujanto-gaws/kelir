/**
 * Where the harness points, and who it signs in as.
 *
 * Read here rather than at each use so a missing variable is one error naming
 * the variable, not a login failing for reasons the report will not explain.
 * Every value has a default matching what `deploy-local.sh` brings up on a
 * laptop, so the common case needs no environment at all.
 */

/** Reads a variable, falling back to what the local deployment uses. */
function read(name: string, fallback: string): string {
  const value = process.env[name]

  return value === undefined || value === '' ? fallback : value
}

/**
 * The address the deployed stack answers on.
 *
 * `127.0.0.1:8080` is `deploy-local.sh`'s default port on the machine that ran
 * it. A deployment reachable by another address — a real host, or CI publishing
 * elsewhere — sets `KELIR_E2E_BASE_URL`.
 */
export function baseUrl(): string {
  return read('KELIR_E2E_BASE_URL', 'http://127.0.0.1:8080').replace(/\/+$/, '')
}

/**
 * The path prefix every API call carries, below that same address — Caddy
 * proxies it to the backend (`deploy/staging/Caddyfile`).
 *
 * A prefix rather than a second base URL, because Playwright resolves a request
 * path against its context's `baseURL` as a URL would: a leading slash discards
 * the path, so a context based at `…/api/v1` sends `/auth/login` to the site
 * root, where Caddy serves the single-page app and a POST comes back 405.
 */
export const API_PREFIX = '/api/v1'

/**
 * The account the flow signs in as.
 *
 * The first-run administrator, because it is the one account a fresh
 * deployment is guaranteed to have — `KELIR_BOOTSTRAP_ADMIN_*` creates it at
 * startup when the `users` table is empty. There is no password there because
 * a default password in a repository is a credential in a repository; the
 * deployment that the harness is pointed at knows its own.
 */
export interface Credentials {
  readonly username: string
  readonly password: string
}

export function credentials(): Credentials {
  const password = process.env.KELIR_E2E_PASSWORD

  if (password === undefined || password === '') {
    throw new Error(
      'KELIR_E2E_PASSWORD is not set. It is the password of the account the ' +
        'flow signs in as — the deployment’s KELIR_BOOTSTRAP_ADMIN_PASSWORD ' +
        'unless KELIR_E2E_USERNAME names another account.',
    )
  }

  return { username: read('KELIR_E2E_USERNAME', 'admin'), password }
}
