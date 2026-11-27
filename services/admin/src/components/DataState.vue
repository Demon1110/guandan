<script setup lang="ts">
defineProps<{
  kind: "loading" | "empty" | "error";
  title: string;
  message?: string;
}>();
</script>

<template>
  <div class="state-panel" :class="`state-panel--${kind}`">
    <span v-if="kind === 'loading'" class="spinner" aria-hidden="true" />
    <span v-else class="state-panel__mark" aria-hidden="true">
      {{ kind === "error" ? "!" : "∅" }}
    </span>
    <strong>{{ title }}</strong>
    <p v-if="message">{{ message }}</p>
    <slot />
  </div>
</template>
