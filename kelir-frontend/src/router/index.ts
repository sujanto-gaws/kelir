import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router'

/**
 * Route table.
 *
 * `meta.requiresAuth` is declared now and enforced in Phase 2, when the auth
 * store exists — declaring it here keeps the guard's contract in one place
 * rather than scattered through the router when it lands.
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

router.afterEach((to) => {
  const title = typeof to.meta.title === 'string' ? to.meta.title : undefined
  document.title = title ? `${title} · Kelir` : 'Kelir'
})

export default router
