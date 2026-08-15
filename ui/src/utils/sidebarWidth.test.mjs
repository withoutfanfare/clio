import assert from "node:assert/strict";
import test from "node:test";

const sidebarWidth = await import("./sidebarWidth.ts");

test("uses the default width when the stored value is missing or invalid", () => {
  assert.equal(sidebarWidth.normaliseSidebarWidth(null), 220);
  assert.equal(sidebarWidth.normaliseSidebarWidth("not-a-width"), 220);
});

test("keeps the sidebar width within its usable range", () => {
  assert.equal(sidebarWidth.normaliseSidebarWidth("120"), 180);
  assert.equal(sidebarWidth.normaliseSidebarWidth("500"), 420);
  assert.equal(sidebarWidth.normaliseSidebarWidth("312"), 312);
});
