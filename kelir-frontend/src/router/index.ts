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
