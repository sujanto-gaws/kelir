<script setup lang="ts">
/**
 * `divider` — a purely presentational type (JFSS §4.4).
 *
 * §4.4 requires these to carry neither `content` nor `calculate`, which is why
 * this component takes no props at all: there is nothing about a divider a
 * definition can vary.
 *
 * `<hr>` rather than a styled div, so the separation is announced and not only
 * drawn.
 */
</script>

<template>
  <hr class="border-border" />
</template>
