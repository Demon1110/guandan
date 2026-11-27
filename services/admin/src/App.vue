<script setup lang="ts">
import { onBeforeUnmount, onMounted } from "vue";
import { useRouter } from "vue-router";
import ToastHost from "./components/ToastHost.vue";

const router = useRouter();

function handleUnauthorized() {
  if (router.currentRoute.value.name !== "login") {
    router.replace({ name: "login", query: { expired: "1" } });
  }
}

onMounted(() => window.addEventListener("gf-admin-unauthorized", handleUnauthorized));
onBeforeUnmount(() => window.removeEventListener("gf-admin-unauthorized", handleUnauthorized));
</script>

<template>
  <RouterView />
  <ToastHost />
</template>
