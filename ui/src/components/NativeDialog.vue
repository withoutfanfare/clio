<script setup lang="ts">
import { ref, watchPostEffect, onBeforeUnmount } from "vue";

const props = defineProps<{ open: boolean; label: string }>();
const emit = defineEmits<{ close: [] }>();
const dialog = ref<HTMLDialogElement | null>(null);
let returnFocus: HTMLElement | null = null;

function restoreFocus() {
  if (returnFocus?.isConnected) returnFocus.focus();
  returnFocus = null;
}

watchPostEffect(() => {
  const element = dialog.value;
  if (!element) return;
  if (props.open && !element.open) {
    returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    element.showModal();
  } else if (!props.open && element.open) {
    element.close();
    restoreFocus();
  }
});

onBeforeUnmount(() => {
  dialog.value?.close();
  restoreFocus();
});
</script>

<template>
  <Teleport to="body">
    <dialog
      ref="dialog"
      class="native-dialog"
      :aria-label="label"
      aria-modal="true"
      @cancel.prevent="emit('close')"
      @click.self="emit('close')"
    >
      <slot v-if="open" />
    </dialog>
  </Teleport>
</template>

<style scoped>
.native-dialog {
  position: fixed;
  inset: 0;
  width: 100vw;
  height: 100dvh;
  max-width: none;
  max-height: none;
  margin: 0;
  padding: 0;
  border: 0;
  background: transparent;
  color: inherit;
}

.native-dialog::backdrop {
  background: rgba(0, 0, 0, 0.6);
  backdrop-filter: blur(2px);
}
</style>
