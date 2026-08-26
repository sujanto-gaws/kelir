import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router'

import { registerGuards } from './guards'

/**
 * Route table.
 *
 * Route meta is the only thing the guard reads: `requiresAuth` for a session
 * and `permission` for a specific grant. A route opts into protection by
 * declaring them, and `guards.ts` needs no edit to follow.
 */
export const routes: RouteRecordRaw[] = [
  {
    path: '/login',
    name: 'login',
    component: () => import('@/features/auth/LoginPage.vue'),
    meta: { requiresAuth: false, layout: 'blank', title: 'Sign in' },
  },
  {
    path: '/forgot-password',
    name: 'forgot-password',
    component: () => import('@/features/auth/ForgotPasswordPage.vue'),
    meta: { requiresAuth: false, layout: 'blank', title: 'Reset your password' },
  },
  {
    // The path the emailed link points at: `KELIR_FRONTEND_URL` +
    // `/reset-password?token=...`, built in `auth::reset`. Changing it here
    // without changing it there strands every link already in a mailbox.
    path: '/reset-password',
    name: 'reset-password',
    component: () => import('@/features/auth/ResetPasswordPage.vue'),
    meta: { requiresAuth: false, layout: 'blank', title: 'Choose a new password' },
  },
  {
    path: '/',
    component: () => import('@/layouts/AppLayout.vue'),
    children: [
      {
        path: '',
        name: 'dashboard',
        component: () => import('@/features/dashboard/DashboardPage.vue'),
        meta: { requiresAuth: true, title: 'Dashboard' },
      },
      {
        // One route over the four lists: the view is a path segment so a
        // supplier list can be linked to, and the search and filters live in
        // the query string beside it (#101).
        path: 'master-data/:view(parties|suppliers|customers|employees)?',
        name: 'master-data',
        component: () => import('@/features/master-data/MasterDataListPage.vue'),
        // The party read permission alone. The role views need
        // `master-data:party-role:read` as well, and the page offers only the
        // tabs the caller may open rather than the route refusing all four for
        // want of one — a caller who may read parties and not roles has a
        // working screen, not a forbidden one (#101 AC5).
        meta: {
          requiresAuth: true,
          permission: 'master-data:party:read',
          title: 'Master data',
        },
      },
      {
        // The form id is the route, not a query parameter: a filled-in form is
        // a place, and Sprint 9's document workspace links to one.
        path: 'forms/:id',
        name: 'form-render',
        component: () => import('@/features/rad/FormRenderPage.vue'),
        // The permission that opens a definition. A lookup field inside the
        // form needs the master-data permission of whatever it points at, and
        // that is enforced where it belongs — on `/rad/lookups/…`, per the
        // reasoning #97 and D-12 have applied before. A caller who may read
        // forms and not suppliers gets the form with one chooser that reports
        // it could not load, not a route that refuses the whole form.
        meta: { requiresAuth: true, permission: 'rad:form:read', title: 'Form' },
      },
      {
        path: 'admin/users',
        name: 'admin-users',
        component: () => import('@/features/admin/UserListPage.vue'),
        meta: { requiresAuth: true, permission: 'identity:user:read', title: 'Users' },
      },
      {
        path: 'admin/roles',
        name: 'admin-roles',
        component: () => import('@/features/admin/RoleListPage.vue'),
        meta: { requiresAuth: true, permission: 'identity:role:read', title: 'Roles' },
      },
      {
        path: 'admin/tenants',
        name: 'admin-tenants',
        component: () => import('@/features/admin/TenantListPage.vue'),
        // The permission alone. The backend also requires the caller to be in
        // the deployment's administering tenant, which no route meta can
        // express — a token carries permissions, not that fact. It does not
        // need to: a tenant's own administrator is never granted this code
        // (decision D-18), so the route is unreachable where it would refuse.
        meta: {
          requiresAuth: true,
          permission: 'organization:tenant:read',
          title: 'Tenants',
        },
      },
      {
        path: 'forbidden',
        name: 'forbidden',
        component: () => import('@/pages/ForbiddenPage.vue'),
        // Inside the shell, and authenticated: the caller has a session, they
        // just lack one grant, so stranding them outside the app would be wrong.
        meta: { requiresAuth: true, title: 'No access' },
      },
    ],
  },
  {
    path: '/:pathMatch(.*)*',
    name: 'not-found',
    component: () => import('@/pages/NotFoundPage.vue'),
    meta: { requiresAuth: false, title: 'Not found' },
  },
]

const router = createRouter({
  history: createWebHistory(),
  routes,
})

registerGuards(router)

router.afterEach((to) => {
  const title = typeof to.meta.title === 'string' ? to.meta.title : undefined
  document.title = title ? `${title} · Kelir` : 'Kelir'
})

export default router
