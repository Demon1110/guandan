import { reactive } from "vue";

export type ToastKind = "success" | "error" | "info";

export interface ToastItem {
  id: number;
  kind: ToastKind;
  message: string;
}

export const toasts = reactive<{ items: ToastItem[] }>({ items: [] });

let nextId = 1;

export function toast(message: string, kind: ToastKind = "success", duration = 3200): void {
  const id = nextId++;
  toasts.items.push({ id, kind, message });
  window.setTimeout(() => dismissToast(id), duration);
}

export function dismissToast(id: number): void {
  const index = toasts.items.findIndex((item) => item.id === id);
  if (index >= 0) toasts.items.splice(index, 1);
}
