import { test } from "node:test";
import assert from "node:assert/strict";
import { api, ApiError } from "./api.ts";

test("a proxy HTML response is never surfaced as monitoring data or an HTML error", async (t) => {
  const original = globalThis.fetch;
  t.after(() => (globalThis.fetch = original));
  globalThis.fetch = async () =>
    new Response("<html>proxy-secret diagnostic</html>", { status: 502 });
  await assert.rejects(
    api("/nodes"),
    (e) =>
      e instanceof ApiError &&
      e.status === 502 &&
      !e.message.includes("proxy-secret"),
  );
  globalThis.fetch = async () =>
    new Response("<html>challenge</html>", { status: 200 });
  await assert.rejects(
    api("/nodes"),
    (e) =>
      e instanceof ApiError && e.message === "返回内容不是监控数据，请重试。",
  );
});
test("cancelled requests preserve cancellation rather than displaying a network error", async (t) => {
  const original = globalThis.fetch;
  t.after(() => (globalThis.fetch = original));
  globalThis.fetch = async () => {
    throw new DOMException("aborted", "AbortError");
  };
  await assert.rejects(
    api("/nodes"),
    (e) => (e as Error).name === "AbortError",
  );
});
test("failed connections have a readable error without exposing implementation details", async (t) => {
  const original = globalThis.fetch;
  t.after(() => (globalThis.fetch = original));
  globalThis.fetch = async () => {
    throw new TypeError("internal fetch implementation");
  };
  await assert.rejects(
    api("/nodes"),
    (e) =>
      e instanceof ApiError &&
      e.status === 0 &&
      !e.message.includes("implementation"),
  );
});
