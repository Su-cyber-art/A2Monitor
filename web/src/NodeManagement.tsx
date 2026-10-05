import { useEffect, useRef, useState } from "react";
import {
  closestCenter, DndContext, DragOverlay, KeyboardSensor, PointerSensor,
  useSensor, useSensors, type DragEndEvent,
} from "@dnd-kit/core";
import { SortableContext, sortableKeyboardCoordinates, useSortable, verticalListSortingStrategy } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { Download, GripVertical, Pencil, Search, Server, Trash2 } from "lucide-react";
import { Badge, Empty, ErrorBox } from "./components";
import { reorderVisibleNodes } from "./node-order";
import type { Node } from "./types";

type Props = {
  nodes: Node[];
  updating: string | null;
  onEdit: (node: Node) => void;
  onInstall: (node: Node) => void;
  onDelete: (node: Node) => void;
  onVisibility: (node: Node) => Promise<void>;
  onReorder: (ids: string[]) => Promise<void>;
};

export function NodeManagement(props: Props) {
  const { nodes } = props;
  const [query, setQuery] = useState("");
  const [group, setGroup] = useState("");
  const [order, setOrder] = useState(nodes.map((n) => n.id));
  const [activeId, setActiveId] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pending = useRef<{ from: string; to: string } | null>(null);
  const serverOrder = nodes.map((n) => n.id).join(",");
  useEffect(() => {
    if (saving || activeId) return;
    if (pending.current && pending.current.to !== serverOrder && pending.current.from === serverOrder) return;
    pending.current = null;
    setOrder(serverOrder.split(",").filter(Boolean));
  }, [serverOrder, saving, activeId]);
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const ordered = [...order.map((id) => byId.get(id)).filter((n): n is Node => !!n),
    ...nodes.filter((n) => !order.includes(n.id))];
  const shown = ordered.filter((n) => (!group || n.group === group) &&
    `${n.name} ${n.group} ${n.region}`.toLowerCase().includes(query.trim().toLowerCase()));
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );
  const drop = async ({ active, over }: DragEndEvent) => {
    setActiveId(null);
    if (!over || active.id === over.id) return;
    const previous = ordered.map((n) => n.id);
    const next = reorderVisibleNodes(previous, shown.map((n) => n.id), String(active.id), String(over.id));
    if (next === previous) return;
    setOrder(next);
    setSaving(true);
    setError(null);
    pending.current = { from: serverOrder, to: next.join(",") };
    try {
      await props.onReorder(next);
    } catch (e) {
      pending.current = null;
      setOrder(previous);
      setError((e as Error).message);
    } finally {
      setSaving(false);
    }
  };
  const active = byId.get(activeId || "");
  return <section className="management-panel">
    <div className="management-toolbar">
      <div className="management-total"><Server size={17} /><strong>{nodes.length}</strong><span>个节点</span>
        {saving && <span className="order-saving" role="status">保存排序…</span>}
      </div>
      <div className="toolbar-actions">
        <select aria-label="筛选节点分组" value={group} onChange={(e) => setGroup(e.target.value)}>
          <option value="">全部分组</option>
          {[...new Set(nodes.map((n) => n.group).filter(Boolean))].map((g) => <option key={g}>{g}</option>)}
        </select>
        <div className="search"><Search size={16} /><input aria-label="搜索节点" placeholder="搜索节点" value={query} onChange={(e) => setQuery(e.target.value)} /></div>
      </div>
    </div>
    <ErrorBox error={error} />
    {shown.length ? <DndContext sensors={sensors} collisionDetection={closestCenter}
      onDragStart={({ active }) => setActiveId(String(active.id))}
      onDragCancel={() => setActiveId(null)} onDragEnd={drop}
      accessibility={{ screenReaderInstructions: { draggable: "按空格开始排序，使用上下方向键移动，再按空格保存，按 Escape 取消。" },
        announcements: {
          onDragStart: ({active}) => `开始移动 ${byId.get(String(active.id))?.name || "节点"}`,
          onDragOver: ({active,over}) => over ? `${byId.get(String(active.id))?.name} 移至第 ${shown.findIndex(n=>n.id===over.id)+1} 位` : "",
          onDragEnd: () => "移动完成", onDragCancel: () => "已取消排序",
        } }}>
      <div className="table-wrap management-table-wrap"><table className="management-table">
        <thead><tr><th className="drag-cell"><span className="sr-only">排序</span></th><th>节点名称</th><th>分组</th><th>连接状态</th><th>公开展示</th><th className="right">操作</th></tr></thead>
        <SortableContext items={shown.map((n) => n.id)} strategy={verticalListSortingStrategy}>
          <tbody>{shown.map((node) => <ManagementRow key={node.id} node={node} {...props} disabled={saving || !!props.updating} />)}</tbody>
        </SortableContext>
      </table></div>
      <DragOverlay>{active && <div className="node-drag-preview"><GripVertical size={18} /><strong>{active.name}</strong><Badge>{active.group || "未分组"}</Badge></div>}</DragOverlay>
    </DndContext> : <Empty title="未找到节点" action={<button className="button small" onClick={()=>{setGroup("");setQuery("");}}>清除筛选</button>} />}
  </section>;
}

function ManagementRow({ node: n, disabled, ...props }: Props & { node: Node; disabled: boolean }) {
  const { attributes, listeners, setNodeRef, setActivatorNodeRef, transform, transition, isDragging } = useSortable({ id:n.id, disabled });
  return <tr ref={setNodeRef} data-node-id={n.id} className={isDragging ? "node-row-dragging" : ""}
    style={{ transform: CSS.Transform.toString(transform), transition }}>
    <td className="drag-cell"><button ref={setActivatorNodeRef} {...attributes} {...listeners} className="drag-handle" aria-label={`拖动排序 ${n.name}`} title="拖动排序" disabled={disabled}><GripVertical size={17} /></button></td>
    <td><button className="management-node" onClick={()=>props.onEdit(n)} disabled={disabled}>
      <span className="management-node-icon">{n.region || <Server size={17} />}</span>
      <span><strong title={n.name}>{n.name}</strong><small>#{String(n.alias).padStart(3,"0")}<span className="mobile-node-group">{n.group || "未分组"}</span></small></span>
    </button></td>
    <td><Badge>{n.group || "未分组"}</Badge></td>
    <td><span className={`connection-pill ${n.online ? (n.fresh ? "online" : "waiting") : "offline"}`}><span />{n.online ? (n.fresh ? "在线" : "等待采样") : "离线"}</span></td>
    <td><button type="button" className="visibility-switch" role="switch" aria-checked={n.public} aria-label={`公开展示 ${n.name}`} disabled={disabled} onClick={()=>void props.onVisibility(n)}><span /></button></td>
    <td className="right"><div className="row-actions">
      <button className="icon-button" aria-label={`编辑 ${n.name}`} title="编辑节点" disabled={disabled} onClick={()=>props.onEdit(n)}><Pencil size={16} /></button>
      <button className="icon-button" aria-label={`安装 ${n.name}`} title="生成安装命令" disabled={disabled} onClick={()=>props.onInstall(n)}><Download size={16} /></button>
      <button className="icon-button destructive" aria-label={`删除 ${n.name}`} title="删除节点" disabled={disabled} onClick={()=>props.onDelete(n)}><Trash2 size={16} /></button>
    </div></td>
  </tr>;
}
