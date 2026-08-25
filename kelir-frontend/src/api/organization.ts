import { deleteItem, getItem, getPage, postItem, putItem } from './client'
import type { Page, PageQuery } from '@/types/api'
import type { CreateTenantRequest, Tenant, UpdateTenantRequest } from '@/types/organization'

/**
 * Tenant administration (`/api/v1/organization/tenants`, FR-ORG-001).
 *
 * Thin by design: every function is one call through the shared client, so
 * envelope unwrapping and error normalisation happen in exactly one place
 * (coding standard §3.3). Nothing here composes a message for a user.
 *
 * **These endpoints carry a second condition no other module's do.** Besides
 * `organization:tenant:read` or `:manage`, the caller must be signed in to the
 * deployment's *administering* tenant — the one `KELIR_DEFAULT_TENANT_CODE`
 * names. A tenant's own administrator holds every other permission in the
 * catalogue and is still refused here, with 403. The `can()` gates in the page
 * cannot express that, so the nav entry and the page both rely on the fact that
 * a tenant administrator is not granted `organization:tenant:*` in the first
 * place; the server is what actually decides.
 */

const TENANTS = '/organization/tenants'

/** Paginated. `page` is 1-based; `pageSize` is clamped server-side to 1..=100. */
export function listTenants(query: PageQuery = {}): Promise<Page<Tenant>> {
  return getPage<Tenant>(TENANTS, query)
}

export function getTenant(id: string): Promise<Tenant> {
  return getItem<Tenant>(`${TENANTS}/${id}`)
}

/**
 * 201 with the tenant and its first administrator, both created in one
 * transaction. 409 when the tenant code — or the administrator's username or
 * email — is already in use; 422 on field problems, whose paths are nested
 * (`administrator.password`).
 */
export function createTenant(request: CreateTenantRequest): Promise<Tenant> {
  return postItem<Tenant>(TENANTS, request)
}

/**
 * Rename, suspend or reactivate. 400 when the target is the tenant the request
 * came from and the change would take it offline.
 *
 * Suspending revokes the tenant's refresh tokens, so its users' sessions end
 * rather than merely failing to renew — an access token already issued stays
 * valid until it expires, up to fifteen minutes.
 */
export function updateTenant(id: string, request: UpdateTenantRequest): Promise<Tenant> {
  return putItem<Tenant>(`${TENANTS}/${id}`, request)
}

/**
 * Soft-delete. Answers 204; 400 for the administering tenant.
 *
 * The tenant's users, roles and data stay in place — what makes them
 * unreachable is that the tenant no longer resolves at sign-in.
 */
export function deleteTenant(id: string): Promise<void> {
  return deleteItem(`${TENANTS}/${id}`)
}
