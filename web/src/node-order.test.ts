import { test } from "node:test";
import assert from "node:assert/strict";
import { reorderVisibleNodes } from "./node-order.ts";

test("dragging in a filtered list preserves nodes outside the filter", () => {
  const all = ["hidden-first", "a", "other-group", "b", "c", "hidden-last"];
  assert.deepEqual(reorderVisibleNodes(all, ["a", "b", "c"], "c", "a"),
    ["hidden-first", "c", "other-group", "a", "b", "hidden-last"]);
  assert.deepEqual(all, ["hidden-first", "a", "other-group", "b", "c", "hidden-last"]);
});

test("dragging down persists the complete server order", () => {
  assert.deepEqual(reorderVisibleNodes(["a", "b", "c"], ["a", "b", "c"], "a", "c"), ["b", "c", "a"]);
});

test("cancelled or stale drag targets leave the order unchanged", () => {
  const all = ["a", "b"];
  assert.equal(reorderVisibleNodes(all, all, "a", "a"), all);
  assert.equal(reorderVisibleNodes(all, all, "deleted", "b"), all);
});
