<script setup lang="ts">
withDefaults(
  defineProps<{
    visible: boolean;
    title: string;
    message: string;
    confirmText?: string;
    cancelText?: string;
    danger?: boolean;
    busy?: boolean;
  }>(),
  {
    confirmText: "确认",
    cancelText: "取消",
    danger: false,
    busy: false,
  },
);

const emit = defineEmits<{ close: []; confirm: [] }>();
</script>

<template>
  <Teleport to="body">
    <Transition name="modal">
      <div v-if="visible" class="modal-root" role="dialog" aria-modal="true" :aria-label="title">
        <div class="modal-mask" @click="emit('close')" />
        <div class="modal-card">
          <div class="modal-card__head">
            <span class="modal-card__icon" :class="{ 'modal-card__icon--danger': danger }">
              {{ danger ? "!" : "?" }}
            </span>
            <h3>{{ title }}</h3>
          </div>
          <p class="modal-card__body">{{ message }}</p>
          <div class="modal-card__actions">
            <button class="btn btn--ghost" type="button" :disabled="busy" @click="emit('close')">
              {{ cancelText }}
            </button>
            <button
              class="btn"
              :class="danger ? 'btn--danger' : 'btn--primary'"
              type="button"
              :disabled="busy"
              @click="emit('confirm')"
            >
              {{ busy ? "处理中…" : confirmText }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
