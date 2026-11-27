<script setup lang="ts">
import { dismissToast, toasts } from "../lib/toast";
</script>

<template>
  <div class="toast-stack" aria-live="polite">
    <TransitionGroup name="toast">
      <div
        v-for="item in toasts.items"
        :key="item.id"
        class="toast"
        :class="`toast--${item.kind}`"
        role="status"
      >
        <span class="toast__mark" aria-hidden="true">
          {{ item.kind === "success" ? "✓" : item.kind === "error" ? "!" : "i" }}
        </span>
        <span class="toast__text">{{ item.message }}</span>
        <button class="toast__close" type="button" aria-label="关闭提示" @click="dismissToast(item.id)">
          ×
        </button>
      </div>
    </TransitionGroup>
  </div>
</template>
