<script setup lang="ts">
import { RouterLink } from 'vue-router'

import { Button } from '@/components/ui/button'

/**
 * Where the route guard sends a signed-in caller who lacks a route's
 * permission. Distinct from 404 on purpose: the page exists, and saying so is
 * what lets the user ask an administrator for the right role instead of
 * assuming a broken link.
 */
</script>

<template>
  <div class="flex min-h-[60vh] flex-col items-center justify-center gap-4 px-4">
    <p class="text-sm font-medium text-muted-foreground">403</p>
    <h1 class="text-2xl font-semibold tracking-tight">You do not have access to this page</h1>
    <p class="max-w-sm text-center text-sm text-muted-foreground">
      Your account is missing the permission this page requires. An administrator can grant it by
      adding the matching role to your account.
    </p>
    <RouterLink to="/">
      <Button variant="outline">Back to the dashboard</Button>
    </RouterLink>
  </div>
</template>
