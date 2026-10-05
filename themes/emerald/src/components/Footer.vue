<script setup lang="ts">
import { computed } from 'vue'
import VisitorInfoCard from '@/components/VisitorInfoCard.vue'
import { useAppStore } from '@/stores/app'

const appStore = useAppStore()
const showIcp = computed(() => appStore.icpEnabled && appStore.icpNumber)
const showPolice = computed(() => appStore.policeEnabled && appStore.policeNumber)
</script>

<template>
  <VisitorInfoCard v-if="appStore.visitorInfoCardEnabled" />
  <footer v-if="showIcp || showPolice" class="flex gap-2 justify-center p-4 text-xs text-muted-foreground">
    <a v-if="showIcp" :href="appStore.icpUrl" target="_blank" rel="noopener noreferrer">{{ appStore.icpNumber }}</a>
    <a v-if="showPolice && appStore.policeUrl" :href="appStore.policeUrl" target="_blank" rel="noopener noreferrer">{{ appStore.policeNumber }}</a>
    <span v-else-if="showPolice">{{ appStore.policeNumber }}</span>
  </footer>
</template>
