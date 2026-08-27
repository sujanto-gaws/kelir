<script setup lang="ts">
defineProps<{
  /** What will eventually be here — "Attachments", "Comments". */
  subject: string
  /** When, named as a phase a reader can look up. */
  arrives: string
}>()

/**
 * A tab that says what will fill it and when (#172 AC4).
 *
 * **Neither an empty tab nor a silent one.** A tab that renders blank tells a
 * user the product is broken; a tab that is missing tells them nothing at all,
 * which is fine — but a tab whose *shape* the workspace already commits to is
 * better named than hidden, because it is what makes adding the real thing an
 * addition rather than a rebuild.
 *
 * **Not `disabled`.** A disabled tab reads as "you may not do this", which is a
 * statement about the reader's permissions and is false here: nobody can open
 * this, because there is nothing behind it yet.
 */
</script>

<template>
  <div
    class="rounded-md border border-dashed border-border p-8 text-center"
    data-testid="empty-tab"
  >
    <p class="font-medium">{{ subject }} are not here yet.</p>
    <p class="mt-1 text-sm text-muted-foreground">They arrive in {{ arrives }}.</p>
  </div>
</template>
