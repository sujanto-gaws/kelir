import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router'

import { registerGuards } from './guards'

/**
 * Route table.
 *
 * `meta.requiresAuth` is the only thing the guard reads: a route opts into
 * protection by declaring it, and `guards.ts` needs no edit to follow.
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

registerGuards(router)

router.afterEach((to) => {
  const title = typeof to.meta.title === 'string' ? to.meta.title : undefined
  document.title = title ? `${title} · Kelir` : 'Kelir'
})

export default router
