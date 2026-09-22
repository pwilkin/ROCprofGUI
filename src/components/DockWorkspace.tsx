import {
  type DockTree,
  reconcileDockTree,
  placeDockPanel,
  updateDockSplit,
  dockHeight,
} from "../dockTree";
import type { ReactNode } from "react";
export interface DockLayout {
  pinned: string[];
  arrangement: "stacked" | "columns" | "custom";
  tree?: DockTree;
  split: number;
}
export const defaultDockLayout = (): DockLayout => ({
  pinned: [],
  arrangement: "stacked",
  split: 50,
});
export default function DockWorkspace({
  active,
  layout,
  onChange: emitChange,
  views,
  children,
}: {
  active: string;
  layout: DockLayout;
  onChange: (layout: DockLayout) => void;
  views: { id: string; name: string }[];
  children: (view: string) => ReactNode;
}) {
  const tree = reconcileDockTree(layout.tree, layout.pinned);
  const onChange = (next: DockLayout) =>
    emitChange({
      ...next,
      ...(next.tree || next.arrangement === "custom"
        ? { tree: reconcileDockTree(next.tree, next.pinned) }
        : {}),
    });
  const panels = [
    ...layout.pinned,
    ...(!layout.pinned.includes(active) ? [active] : []),
  ];
  const move = (index: number, offset: number) => {
    const pinned = [...layout.pinned];
    [pinned[index], pinned[index + offset]] = [
      pinned[index + offset],
      pinned[index],
    ];
    onChange({ ...layout, pinned });
  };
  const renderPanel = (view: string, index: number) => (
    <section
      className="dock-panel"
      key={view}
      aria-label={`${views.find((v) => v.id === view)?.name || view} panel`}
    >
      <header className="dock-heading">
        <strong>{views.find((v) => v.id === view)?.name || view}</strong>
        {layout.pinned.includes(view) ? (
          <div>
            {layout.arrangement === "custom" && tree && (
              <select
                aria-label={`Place ${view} panel`}
                value=""
                onChange={(e) => {
                  if (!e.target.value) return;
                  const [position, target] = e.target.value.split(":");
                  onChange({
                    ...layout,
                    tree: placeDockPanel(tree, view, target, position),
                  });
                }}
              >
                <option value="">Place panel…</option>
                {layout.pinned
                  .filter((id) => id !== view)
                  .flatMap((target) =>
                    ["left", "right", "above", "below"].map((position) => (
                      <option
                        key={`${position}:${target}`}
                        value={`${position}:${target}`}
                      >
                        {position} of{" "}
                        {views.find((v) => v.id === target)?.name || target}
                      </option>
                    )),
                  )}
              </select>
            )}
            <button
              aria-label={`Move ${view} panel earlier`}
              disabled={index === 0 || layout.arrangement === "custom"}
              onClick={() => move(index, -1)}
            >
              ↑
            </button>
            <button
              aria-label={`Move ${view} panel later`}
              disabled={
                index >= layout.pinned.length - 1 ||
                layout.arrangement === "custom"
              }
              onClick={() => move(index, 1)}
            >
              ↓
            </button>
            <button
              aria-label={`Unpin ${view} panel`}
              onClick={() =>
                onChange({
                  ...layout,
                  pinned: layout.pinned.filter((id) => id !== view),
                })
              }
            >
              Unpin
            </button>
          </div>
        ) : (
          <span>Active view</span>
        )}
      </header>
      <div className="dock-panel-body">{children(view)}</div>
    </section>
  );
  const renderTree = (node: DockTree, path = ""): ReactNode => {
    if (node.kind === "panel")
      return renderPanel(node.view, layout.pinned.indexOf(node.view));
    return (
      <div className="dock-split" key={path}>
        <div className="dock-split-controls">
          <select
            aria-label={`Split direction ${path || "root"}`}
            value={node.axis}
            onChange={(e) =>
              onChange({
                ...layout,
                tree: updateDockSplit(tree!, path, {
                  axis: e.target.value as "horizontal" | "vertical",
                }),
              })
            }
          >
            <option value="horizontal">Side by side</option>
            <option value="vertical">Above / below</option>
          </select>
          <input
            aria-label={`Split proportion ${path || "root"}`}
            type="range"
            min="20"
            max="80"
            value={node.ratio}
            onChange={(e) =>
              onChange({
                ...layout,
                tree: updateDockSplit(tree!, path, {
                  ratio: Number(e.target.value),
                }),
              })
            }
          />
          <span>{node.ratio}%</span>
        </div>
        <div
          className={`dock-branches ${node.axis}`}
          style={
            node.axis === "horizontal"
              ? {
                  gridTemplateColumns: `minmax(0, ${node.ratio}fr) minmax(0, ${100 - node.ratio}fr)`,
                }
              : {
                  gridTemplateRows: `minmax(0, ${node.ratio}fr) minmax(0, ${100 - node.ratio}fr)`,
                }
          }
        >
          {renderTree(node.first, path + "0")}
          {renderTree(node.second, path + "1")}
        </div>
      </div>
    );
  };
  return (
    <>
      <div className="dock-toolbar" aria-label="Workspace layout">
        <button
          disabled={layout.pinned.includes(active)}
          onClick={() =>
            onChange({ ...layout, pinned: [...layout.pinned, active] })
          }
        >
          Pin current view
        </button>
        <label>
          Add panel{" "}
          <select
            aria-label="Add docked panel"
            value=""
            onChange={(e) => {
              if (e.target.value)
                onChange({
                  ...layout,
                  pinned: [...layout.pinned, e.target.value],
                });
            }}
          >
            <option value="">Choose a view…</option>
            {views
              .filter((view) => !layout.pinned.includes(view.id))
              .map((view) => (
                <option key={view.id} value={view.id}>
                  {view.name}
                </option>
              ))}
          </select>
        </label>
        {!!layout.pinned.length && (
          <>
            <label>
              Arrangement{" "}
              <select
                aria-label="Panel arrangement"
                value={layout.arrangement}
                onChange={(e) =>
                  onChange({
                    ...layout,
                    arrangement: e.target.value as DockLayout["arrangement"],
                  })
                }
              >
                <option value="stacked">Stacked</option>
                <option value="columns">Side by side</option>
                <option value="custom">Nested splits</option>
              </select>
            </label>
            {layout.arrangement === "columns" && (
              <label>
                Left column{" "}
                <input
                  aria-label="Left column width"
                  type="range"
                  min="25"
                  max="75"
                  value={layout.split}
                  onChange={(e) =>
                    onChange({ ...layout, split: Number(e.target.value) })
                  }
                />
                {layout.split}%
              </label>
            )}
            <button onClick={() => onChange({ ...layout, pinned: [] })}>
              Unpin all views
            </button>
          </>
        )}
      </div>
      {!layout.pinned.length ? (
        children(active)
      ) : layout.arrangement === "custom" && tree ? (
        <div className="dock-custom">
          <div className="dock-tree" style={{ height: dockHeight(tree) }}>
            {renderTree(tree)}
          </div>
          {!layout.pinned.includes(active) && renderPanel(active, -1)}
        </div>
      ) : (
        <div
          className={`dock-panels ${layout.arrangement}`}
          style={{
            gridTemplateColumns:
              layout.arrangement === "columns"
                ? `minmax(0, ${layout.split}fr) minmax(0, ${100 - layout.split}fr)`
                : undefined,
          }}
        >
          {panels.map((view, index) => renderPanel(view, index))}
        </div>
      )}
    </>
  );
}
