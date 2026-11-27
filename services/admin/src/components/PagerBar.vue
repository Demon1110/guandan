<script setup lang="ts">
import { computed } from "vue";
import { formatNumber } from "../lib/format";

const props = defineProps<{
  page: number;
  pageSize: number;
  total: number;
}>();

const emit = defineEmits<{ change: [page: number] }>();

const totalPages = computed(() => Math.max(1, Math.ceil(props.total / props.pageSize)));
const rangeStart = computed(() => (props.total === 0 ? 0 : (props.page - 1) * props.pageSize + 1));
const rangeEnd = computed(() => Math.min(props.total, props.page * props.pageSize));
</script>

<template>
  <div class="pagination">
    <span class="pagination__meta">
      {{ formatNumber(rangeStart) }}–{{ formatNumber(rangeEnd) }} / 共 {{ formatNumber(total) }} 条
    </span>
    <div class="pagination__actions">
      <button
        class="btn btn--ghost btn--sm"
        type="button"
        :disabled="page <= 1"
        @click="emit('change', page - 1)"
      >
        上一页
      </button>
      <span class="pagination__page">{{ page }} / {{ totalPages }}</span>
      <button
        class="btn btn--ghost btn--sm"
        type="button"
        :disabled="page >= totalPages"
        @click="emit('change', page + 1)"
      >
        下一页
      </button>
    </div>
  </div>
</template>
