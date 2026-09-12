import { getItem } from './client'
import type { DashboardSummary } from '@/types/reporting'

/**
 * The dashboard (`/api/v1/dashboard/*`, FR-RPT-001).
 *
 * Thin, like `activity.ts` — one call and an ordinary envelope.
 *
 * **One function, and the later widgets do not add a second.**
 * [ADR-0039](../../../docs/architectures/adr/0039.%20A%20Dashboard%20Widget%20Is%20a%20Purpose-Built%20Endpoint.md)
 * (**D-78**) makes the dashboard one screen with one contract: FR-RPT-002 and
 * FR-RPT-003 extend `DashboardSummary`, so what changes here is the type rather
 * than the number of requests. A `getDashboardTasks` appearing beside this is
 * the decision being reversed in a diff.
 */

/**
 * The caller's own waiting work.
 *
 * **Behind `reporting:dashboard:read`, and the caller may not hold it.** The
 * dashboard is the home route and is deliberately reachable by every signed-in
 * user (`router/index.spec.ts`'s `PERMISSION_EXEMPT`), so the permission gates
 * the *summary* rather than the page — a person without it still has somewhere
 * to land. The caller is what checks first; this function does not, because an
 * API module that decided who may call it would be a second answer to a
 * question the server already answers, and the wrong layer to put it in.
 */
export function getDashboardSummary(): Promise<DashboardSummary> {
  return getItem<DashboardSummary>('/dashboard/summary')
}
