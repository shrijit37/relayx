import { useCallback, useState } from "react";
import { BaseEdge, getBezierPath, useReactFlow, type EdgeProps } from "@xyflow/react";
import { cn } from "@/lib/utils";

export const edgeTypes = { deletable: DeletableEdge };

function DeletableEdge({
  id,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  style,
  markerEnd,
}: EdgeProps) {
  const { deleteElements } = useReactFlow();
  const [hovered, setHovered] = useState(false);

  const [edgePath, mx, my] = getBezierPath({
    sourceX,
    sourceY,
    targetX,
    targetY,
    sourcePosition,
    targetPosition,
  });

  const onDelete = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      deleteElements({ edges: [{ id }] });
    },
    [deleteElements, id],
  );

  return (
    <g
      className="react-flow__edge-deletable"
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <BaseEdge
        id={id}
        path={edgePath}
        style={style}
        interactionWidth={20}
        {...(markerEnd ? { markerEnd } : {})}
      />
      <foreignObject
        x={mx - 10}
        y={my - 10}
        width={20}
        height={20}
        requiredExtensions="http://www.w3.org/1999/xhtml"
        className={cn(!hovered && "pointer-events-none")}
        style={{ opacity: hovered ? 1 : 0 }}
      >
        <button
          onClick={onDelete}
          onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") onDelete(e as unknown as React.MouseEvent); }}
          tabIndex={0}
          style={{
            width: 20,
            height: 20,
            borderRadius: "50%",
            border: "1px solid var(--color-border-strong)",
            background: "var(--color-panel)",
            color: "var(--color-muted-foreground)",
            display: "grid",
            placeItems: "center",
            cursor: "pointer",
            padding: 0,
            lineHeight: 0,
            pointerEvents: "all",
            boxShadow: "0 1px 3px oklch(0 0 0 / 20%)",
          }}
          aria-label="Delete connection"
          title="Delete connection"
        >
          <svg
            width="10"
            height="10"
            viewBox="0 0 10 10"
            stroke="currentColor"
            strokeWidth="1.5"
            fill="none"
            strokeLinecap="round"
          >
            <line x1="2" y1="2" x2="8" y2="8" />
            <line x1="8" y1="2" x2="2" y2="8" />
          </svg>
        </button>
      </foreignObject>
    </g>
  );
}
