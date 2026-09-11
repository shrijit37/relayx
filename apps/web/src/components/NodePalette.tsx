// Node palette — sidebar for dragging nodes onto the canvas

import { NODE_REGISTRY, type NodeTypeInfo } from "../lib/node-registry";
import { useWorkflowStore } from "../store/workflow-store";

function PaletteItem({ info }: { info: NodeTypeInfo }) {
  const addNode = useWorkflowStore((s) => s.addNode);

  return (
    <button
      className="palette-item"
      onClick={() => addNode(info.kind)}
      title={info.description}
    >
      <span className="palette-icon">{info.icon}</span>
      <span className="palette-label">{info.label}</span>
    </button>
  );
}

export function NodePalette() {
  const categories = {
    flow: NODE_REGISTRY.filter((n) => n.category === "flow"),
    llm: NODE_REGISTRY.filter((n) => n.category === "llm"),
    integration: NODE_REGISTRY.filter((n) => n.category === "integration"),
  };

  return (
    <div className="node-palette">
      {Object.entries(categories).map(([cat, items]) => (
        <div key={cat} className="palette-section">
          <h4 className="palette-category">{cat}</h4>
          {items.map((info) => (
            <PaletteItem key={info.kind} info={info} />
          ))}
        </div>
      ))}
    </div>
  );
}