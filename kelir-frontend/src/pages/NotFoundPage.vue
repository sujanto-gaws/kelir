<script setup lang="ts">
import { RouterLink } from 'vue-router'

import { Button } from '@/components/ui/button'
</script>

<template>
  <div class="flex min-h-screen flex-col items-center justify-center gap-4 bg-background px-4">
    <p class="text-sm font-medium text-muted-foreground">404</p>
    <h1 class="text-2xl font-semibold tracking-tight">Page not found</h1>
    <p class="max-w-sm text-center text-sm text-muted-foreground">
      The page you were looking for does not exist, or you may not have access to it.
    </p>
    <RouterLink to="/">
      <Button variant="outline">Back to the dashboard</Button>
    </RouterLink>
  </div>
</template>
