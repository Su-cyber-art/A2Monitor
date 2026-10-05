export function reorderVisibleNodes(
  all: string[],
  visible: string[],
  active: string,
  over: string,
): string[] {
  const from = visible.indexOf(active);
  const to = visible.indexOf(over);
  if (from < 0 || to < 0 || from === to) return all;
  const moved = [...visible];
  moved.splice(to, 0, moved.splice(from, 1)[0]);
  const shown = new Set(visible);
  let index = 0;
  return all.map((id) => (shown.has(id) ? moved[index++] : id));
}
