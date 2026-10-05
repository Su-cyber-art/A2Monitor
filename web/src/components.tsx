import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { AlertCircle, Check, Copy, Inbox, X } from "lucide-react";
import { date } from "./api";

export function Field({
  label,
  children,
  help,
}: {
  label: string;
  children: ReactNode;
  help?: string;
}) {
  return (
    <label className="field">
      <span>{label}</span>
      {children}
      {help && <small>{help}</small>}
    </label>
  );
}
export function ErrorBox({ error }: { error: string | null | undefined }) {
  return error ? (
    <div className="error-box" role="alert">
      <AlertCircle size={17} />
      <span>{error}</span>
    </div>
  ) : null;
}
export function Empty({
  title,
  detail,
  action,
}: {
  title: string;
  detail?: string;
  action?: ReactNode;
}) {
  return (
    <div className="empty">
      <div className="empty-icon">
        <Inbox size={27} />
      </div>
      <h3>{title}</h3>
      {detail && <p>{detail}</p>}
      {action}
    </div>
  );
}
export function Modal({
  title,
  children,
  onClose,
  wide = false,
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
  wide?: boolean;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
    return () => dialog?.close();
  }, []);
  return createPortal(
    <dialog
      ref={ref}
      className={`dialog ${wide ? "wide" : ""}`}
      aria-label={title}
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      onClick={(e) => {
        if (e.target === ref.current) {
          const r = ref.current.getBoundingClientRect();
          if (
            e.clientX < r.left ||
            e.clientX > r.right ||
            e.clientY < r.top ||
            e.clientY > r.bottom
          )
            onClose();
        }
      }}
    >
      <div className="dialog-head">
        <h2>{title}</h2>
        <button
          className="icon-button"
          aria-label="关闭对话框"
          onClick={onClose}
        >
          <X size={20} />
        </button>
      </div>
      {children}
    </dialog>,
    document.body,
  );
}
export function Confirm({
  title,
  detail,
  onClose,
  onConfirm,
}: {
  title: string;
  detail: string;
  onClose: () => void;
  onConfirm: () => Promise<void>;
}) {
  const [busy, setBusy] = useState(false),
    [error, setError] = useState<string | null>(null);
  return (
    <Modal title={title} onClose={onClose}>
      <p className="dialog-copy">{detail}</p>
      <ErrorBox error={error} />
      <div className="dialog-actions">
        <button className="button" onClick={onClose}>
          取消
        </button>
        <button
          className="button danger"
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            try {
              await onConfirm();
              onClose();
            } catch (e) {
              setError((e as Error).message);
            } finally {
              setBusy(false);
            }
          }}
        >
          {busy ? "处理中…" : "确认"}
        </button>
      </div>
    </Modal>
  );
}
export function CopyButton({
  value,
  label = "复制",
}: {
  value: string;
  label?: string;
}) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      className="button small"
      onClick={async () => {
        await navigator.clipboard.writeText(value);
        setCopied(true);
        setTimeout(() => setCopied(false), 2000);
      }}
    >
      {copied ? <Check size={14} /> : <Copy size={14} />}{" "}
      {copied ? "已复制" : label}
    </button>
  );
}
export function Meter({
  value,
  muted = false,
}: {
  value: number | null;
  muted?: boolean;
}) {
  const safe = value === null ? 0 : Math.max(0, Math.min(100, value));
  return (
    <div className={`meter ${muted ? "muted" : ""}`}>
      <span
        style={{
          width: `${safe}%`,
          background:
            safe > 90 ? "var(--danger)" : safe > 75 ? "var(--warning)" : undefined,
        }}
      />
    </div>
  );
}
export function Badge({
  children,
  tone = "neutral",
}: {
  children: ReactNode;
  tone?: string;
}) {
  return <span className={`badge ${tone}`}>{children}</span>;
}
export function NumberStat({
  label,
  value,
  hint,
  icon,
}: {
  label: string;
  value: ReactNode;
  hint?: ReactNode;
  icon?: ReactNode;
}) {
  return (
    <div className="number-stat">
      <div className="stat-label">
        {label}
        {icon}
      </div>
      <div className="stat-value">{value}</div>
      {hint !== undefined && hint !== null && hint !== false && hint !== "" && <div className="stat-hint">{hint}</div>}
    </div>
  );
}

function metric(record: unknown, key: string): number | null {
  const groups: Record<string, string> = {cpu:"cpu",cpu_max:"cpu",cpu_steal:"cpu",mem_used:"memory",mem_total:"memory",swap_used:"swap",disk_used:"disk",disk_total:"disk",net_rx:"network",net_tx:"network",net_rx_max:"network",net_tx_max:"network",tcp:"connections",udp:"connections",procs:"processes"};
  if (record && typeof record === "object" && "available" in record && Array.isArray(record.available) && groups[key] && !record.available.includes(groups[key])) return null;
  let value: unknown = record;
  for (const part of key.split(".")) {
    if (!value || typeof value !== "object") return null;
    value = (value as Record<string, unknown>)[part];
  }
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}
type Series = { key: string; label: string; color: string; dash?: string };
export function Chart({
  data,
  series,
  max,
  format = (v) => v.toFixed(1),
  timezone = "Asia/Taipei",
  height = 195,
}: {
  data: unknown[];
  series: Series[];
  max?: number;
  format?: (v: number) => string;
  timezone?: string;
  height?: number;
}) {
  const id = useId().replaceAll(":", ""),
    [hover, setHover] = useState<number | null>(null);
  const W = 760,
    H = 190,
    L = 48,
    R = 15,
    T = 14,
    B = 25;
  const times = data.map((r) => metric(r, "time") ?? 0),
    first = times[0] ?? 0,
    last = times.at(-1) ?? first + 1;
  const span = Math.max(last - first, 1);
  const values = data.flatMap((r) =>
    series.map((s) => metric(r, s.key)).filter((v): v is number => v !== null),
  );
  const top = max ?? Math.max(1, ...values) * 1.12;
  const x = (i: number) =>
      L + (W - L - R) * (times.length === 1 ? 0.5 : (times[i] - first) / span),
    y = (v: number) => T + (H - T - B) * (1 - Math.min(v / top, 1));
  const step = data.length > 1 ? span / (data.length - 1) : 60000;
  return (
    <div className="chart">
      <div className="chart-legend">
        {series.map((s) => (
          <span key={s.key}>
            <i style={{ background: s.color }} />
            {s.label}
          </span>
        ))}
      </div>
      <div
        className="chart-drawing"
        style={{ height }}
        onMouseLeave={() => setHover(null)}
        onMouseMove={(e) => {
          if (!data.length) return;
          const rect = e.currentTarget.getBoundingClientRect();
          const target =
            first +
            Math.max(
              0,
              Math.min(
                1,
                (((e.clientX - rect.left) / rect.width) * W - L) / (W - L - R),
              ),
            ) *
              span;
          let best = 0;
          for (let i = 1; i < times.length; i++)
            if (Math.abs(times[i] - target) < Math.abs(times[best] - target))
              best = i;
          setHover(best);
        }}
      >
        <svg
          viewBox={`0 0 ${W} ${H}`}
          preserveAspectRatio="none"
          style={{ visibility: data.length ? undefined : "hidden" }}
          role="img"
          aria-label={series.map((s) => s.label).join("、") + "历史曲线"}
        >
          <defs>
            {series.map((s) => (
              <linearGradient
                key={s.key}
                id={`${id}-${s.key.replaceAll(".", "-")}`}
                x1="0"
                y1="0"
                x2="0"
                y2="1"
              >
                <stop offset="0%" stopColor={s.color} stopOpacity=".16" />
                <stop offset="100%" stopColor={s.color} stopOpacity="0" />
              </linearGradient>
            ))}
          </defs>
          {[0, 0.25, 0.5, 0.75, 1].map((p) => (
            <g key={p}>
              <line
                x1={L}
                x2={W - R}
                y1={y(top * p)}
                y2={y(top * p)}
                stroke="#e5e5e5"
                strokeDasharray="3 5"
              />
              <text
                x={L - 8}
                y={y(top * p) + 3}
                textAnchor="end"
                fill="#737373"
                fontSize="9"
              >
                {format(top * p)}
              </text>
            </g>
          ))}
          {series.map((s) => {
            const segments: number[][] = [];
            let current: number[] = [];
            for (let i = 0; i < data.length; i++) {
              if (
                metric(data[i], s.key) === null ||
                (i > 0 && times[i] - times[i - 1] > step * 2.5)
              ) {
                if (current.length) segments.push(current);
                current = [];
              }
              if (metric(data[i], s.key) !== null) current.push(i);
            }
            if (current.length) segments.push(current);
            return (
              <g key={s.key}>
                {segments.map((segment, j) => {
                  const path = segment
                    .map(
                      (i, k) =>
                        `${k ? "L" : "M"}${x(i)},${y(metric(data[i], s.key)!)}`,
                    )
                    .join(" ");
                  return (
                    <g key={j}>
                      <path
                        d={`${path} L${x(segment.at(-1)!)},${H - B} L${x(segment[0])},${H - B} Z`}
                        fill={`url(#${id}-${s.key.replaceAll(".", "-")})`}
                      />
                      <path
                        d={path}
                        fill="none"
                        stroke={s.color}
                        strokeWidth="2"
                        strokeDasharray={s.dash}
                        vectorEffect="non-scaling-stroke"
                      />
                      {segment.length === 1 && (
                        <circle
                          cx={x(segment[0])}
                          cy={y(metric(data[segment[0]], s.key)!)}
                          r="3"
                          fill={s.color}
                        />
                      )}
                    </g>
                  );
                })}
              </g>
            );
          })}
          {data.length > 0 &&
            [0, Math.floor((data.length - 1) / 2), data.length - 1]
              .filter((v, i, a) => a.indexOf(v) === i)
              .map((i) => (
                <text
                  key={i}
                  x={x(i)}
                  y={H - 5}
                  textAnchor={
                    i === 0 ? "start" : i === data.length - 1 ? "end" : "middle"
                  }
                  fill="#737373"
                  fontSize="9"
                >
                  {new Intl.DateTimeFormat("zh-CN", {
                    hour: "2-digit",
                    minute: "2-digit",
                    hour12: false,
                    timeZone: timezone,
                  }).format(times[i])}
                </text>
              ))}
          {hover !== null && hover < data.length && (
            <line
              x1={x(hover)}
              x2={x(hover)}
              y1={T}
              y2={H - B}
              stroke="#a3a3a3"
              strokeDasharray="3 3"
            />
          )}
        </svg>
        {!data.length && (
          <div className="chart-empty">
            暂无数据
          </div>
        )}
        {hover !== null && hover < data.length && (
          <div
            className="chart-tooltip"
            style={{
              left: `${Math.min(72, Math.max(4, (x(hover) / W) * 100))}%`,
            }}
          >
            <small>{date(times[hover], timezone)}</small>
            {series.map((s) => (
              <div key={s.key}>
                <i style={{ background: s.color }} />
                {s.label}
                <strong>
                  {metric(data[hover], s.key) === null
                    ? "不可用"
                    : format(metric(data[hover], s.key)!)}
                </strong>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
