import { onMounted, onUnmounted, ref } from "vue";
import { workOverview, type WorkOverview } from "@/api/work";

export function useWorkOverview() {
  const overview = ref<WorkOverview | null>(null);
  const loading = ref(true);
  const error = ref<string | null>(null);
  const checkedAt = ref<number | null>(null);
  let mounted = false;
  let pending = false;
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function refresh() {
    if (!mounted || pending) return;
    clearTimeout(timer);
    pending = true;
    loading.value = true;
    try {
      const result = await workOverview();
      if (!mounted) return;
      overview.value = result;
      checkedAt.value = Date.now();
      error.value = null;
    } catch (e) {
      if (mounted) error.value = String(e);
    } finally {
      pending = false;
      if (mounted) {
        loading.value = false;
        timer = setTimeout(() => void refresh(), 10_000);
      }
    }
  }

  onMounted(() => { mounted = true; void refresh(); });
  onUnmounted(() => { mounted = false; clearTimeout(timer); });
  return { overview, loading, error, checkedAt, refresh };
}
