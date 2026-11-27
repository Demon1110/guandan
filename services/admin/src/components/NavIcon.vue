<script setup lang="ts">
defineProps<{ name: string }>();
</script>

<template>
  <svg class="nav-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
    <template v-if="name === 'overview'">
      <rect x="3.5" y="3.5" width="7" height="7" rx="1.6" />
      <rect x="13.5" y="3.5" width="7" height="7" rx="1.6" />
      <rect x="3.5" y="13.5" width="7" height="7" rx="1.6" />
      <rect x="13.5" y="13.5" width="7" height="7" rx="1.6" />
    </template>
    <template v-else-if="name === 'users'">
      <circle cx="9" cy="8" r="3.2" />
      <path d="M3.5 19.5c.4-3.2 2.6-5 5.5-5s5.1 1.8 5.5 5" />
      <path d="M15.5 5.6a3 3 0 0 1 0 5.6M16.5 14.4c2.1.4 3.5 2.1 3.8 4.6" />
    </template>
    <template v-else-if="name === 'games'">
      <path d="M4 6.5h16M4 12h16M4 17.5h16" />
      <circle cx="9" cy="6.5" r="2.2" />
      <circle cx="15" cy="12" r="2.2" />
      <circle cx="7" cy="17.5" r="2.2" />
    </template>
    <template v-else-if="name === 'matches'">
      <path d="M6 4.5h9l3.5 3.5v11.5H6z" />
      <path d="M9 9.5h6M9 13h6M9 16.5h3.5" />
    </template>
    <template v-else>
      <circle cx="12" cy="12" r="8.2" />
      <path d="M12 7.5V12l3 2" />
    </template>
  </svg>
</template>
