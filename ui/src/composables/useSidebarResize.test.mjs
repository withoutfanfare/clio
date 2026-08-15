import assert from "node:assert/strict";
import test from "node:test";

const sidebarResize = await import("./useSidebarResize.ts");

function createStorage(initial = {}) {
  const values = new Map(Object.entries(initial));

  return {
    getItem(key) {
      return values.get(key) ?? null;
    },
    setItem(key, value) {
      values.set(key, value);
    },
  };
}

function createPointerEvent(clientX, target) {
  return {
    button: 0,
    clientX,
    currentTarget: target,
    pointerId: 1,
    preventDefault() {},
  };
}

function createResizeHandle() {
  return {
    setPointerCapture() {},
    hasPointerCapture() {
      return true;
    },
    releasePointerCapture() {},
  };
}

test("dragging resizes the sidebar and restores that width next time", () => {
  const storage = createStorage({ "clio-sidebar-width": "260" });
  const resizer = sidebarResize.useSidebarResize(storage);
  const handle = createResizeHandle();

  resizer.startResize(createPointerEvent(100, handle));
  resizer.resize(createPointerEvent(160, handle));
  resizer.stopResize(createPointerEvent(160, handle));

  assert.equal(resizer.width.value, 320);
  assert.equal(sidebarResize.useSidebarResize(storage).width.value, 320);
});

test("losing pointer capture ends resizing and saves the current width", () => {
  const storage = createStorage({ "clio-sidebar-width": "260" });
  const resizer = sidebarResize.useSidebarResize(storage);
  const handle = createResizeHandle();

  resizer.startResize(createPointerEvent(100, handle));
  resizer.resize(createPointerEvent(150, handle));
  assert.equal(resizer.isResizing.value, true);

  assert.doesNotThrow(() => {
    resizer.stopResizeAfterCaptureLoss();
  });

  assert.equal(resizer.isResizing.value, false);
  assert.equal(sidebarResize.useSidebarResize(storage).width.value, 310);
});

test("arrow keys resize in ten-pixel steps and respect the minimum", () => {
  const storage = createStorage({ "clio-sidebar-width": "180" });
  const resizer = sidebarResize.useSidebarResize(storage);

  resizer.resizeWithKeyboard({ key: "ArrowLeft", preventDefault() {} });
  assert.equal(resizer.width.value, 180);

  resizer.resizeWithKeyboard({ key: "ArrowRight", preventDefault() {} });
  assert.equal(resizer.width.value, 190);
  assert.equal(sidebarResize.useSidebarResize(storage).width.value, 190);
});

test("resizing remains available when browser storage is unavailable", () => {
  let resizer;
  assert.doesNotThrow(() => {
    resizer = sidebarResize.useSidebarResize();
  });

  assert.equal(resizer.width.value, 220);
  resizer.resizeWithKeyboard({ key: "ArrowRight", preventDefault() {} });
  assert.equal(resizer.width.value, 230);
});
