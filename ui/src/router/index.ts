import { createRouter, createWebHashHistory } from "vue-router";

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: "/",
      name: "home",
      component: () => import("@/views/HomeView.vue"),
    },
    {
      path: "/memory/:id",
      name: "memory-detail",
      component: () => import("@/views/HomeView.vue"),
    },
    {
      path: "/stats",
      name: "stats",
      component: () => import("@/views/StatsView.vue"),
    },
    {
      path: "/namespaces",
      name: "namespaces",
      component: () => import("@/views/NamespacesView.vue"),
    },
    {
      path: "/tools",
      name: "tools",
      component: () => import("@/views/ToolsView.vue"),
    },
  ],
});

export default router;
