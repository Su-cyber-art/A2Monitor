import { useCallback, useEffect, useState } from "react";
import { api } from "./api";
import type { Node } from "./types";
export function useResource<T>(path: string | null, interval = 0) {
  const [value, setValue] = useState<T | null>(null),
    [error, setError] = useState<string | null>(null),
    [revision, setRevision] = useState(0);
  const refresh = useCallback(() => setRevision((v) => v + 1), []);
  useEffect(() => {
    if (!path) return;
    let stopped = false;
    const controller = new AbortController();
    const load = async () => {
      try {
        const data = await api<T>(path, { signal: controller.signal });
        if (!stopped) {
          setValue(data);
          setError(null);
        }
      } catch (e) {
        if (!stopped && !controller.signal.aborted)
          setError((e as Error).message);
      }
    };
    void load();
    const timer = interval ? setInterval(load, interval) : undefined;
    return () => {
      stopped = true;
      controller.abort();
      clearInterval(timer);
    };
  }, [path, interval, revision]);
  return { value, error, refresh, setValue };
}
export function useLiveNodes(enabled: boolean, admin: boolean) {
  const [nodes, setNodes] = useState<Node[]>([]),
    [error, setError] = useState<string | null>(null),
    [status, setStatus] = useState("connecting"),
    [revision, setRevision] = useState(0);
  const refresh = useCallback(() => setRevision((v) => v + 1), []);
  useEffect(() => {
    if (!enabled) return;
    let stopped = false,
      socket: WebSocket | null = null,
      poll: ReturnType<typeof setInterval> | undefined,
      retry: ReturnType<typeof setTimeout> | undefined;
    const controller = new AbortController();
    const fetchOnce = async () => {
      try {
        const list = await api<Node[]>("/nodes", { signal: controller.signal });
        if (!stopped) {
          setNodes(list);
          setError(null);
        }
      } catch (e) {
        if (!stopped && !controller.signal.aborted)
          setError((e as Error).message);
      }
    };
    void fetchOnce();
    const connect = () => {
      if (stopped) return;
      socket = new WebSocket(
        `${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/api/v1/stream`,
      );
      socket.onmessage = (e) => {
        try {
          const data = JSON.parse(e.data);
          if (!stopped) {
            setNodes(data.nodes);
            setError(null);
            setStatus("live");
            clearInterval(poll);
            poll = undefined;
          }
        } catch {
          setError("实时数据格式无效");
        }
      };
      socket.onerror = () => socket?.close();
      socket.onclose = () => {
        if (stopped) return;
        setStatus("polling");
        poll ??= setInterval(fetchOnce, 5000);
        retry = setTimeout(connect, 5000);
      };
    };
    connect();
    return () => {
      stopped = true;
      controller.abort();
      socket?.close();
      clearInterval(poll);
      clearTimeout(retry);
    };
  }, [enabled, admin, revision]);
  return { nodes, error, status, refresh };
}
