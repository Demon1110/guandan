<script setup lang="ts">
defineProps<{
  tone: "active" | "disabled" | "on" | "off" | "neutral";
  label: string;
  dot?: boolean;
}>();
</script>

<template>
  <span class="pill" :class="`pill--${tone}`">
    <span v-if="dot" class="pill__dot" aria-hidden="true" />
    {{ label }}
  </span>
</template>
