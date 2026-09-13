import type { Edge } from "@xyflow/react";
import type { RelayNode } from "./nodes";

/** Empty defaults for a new workflow — input on the left, output on the right. */
export const defaultNodes: RelayNode[] = [
  {
    id: "input",
    type: "relay",
    position: { x: 0, y: 0 },
    data: {
      kind: "input",
      title: "HTTP Request",
      lines: ["OpenAI Responses", "POST /v1/responses"],
      metaLeft: "ingress",
    },
  },
  {
    id: "output",
    type: "relay",
    position: { x: 560, y: 0 },
    data: {
      kind: "output",
      title: "Streaming Response",
      lines: ["SSE · text/event-stream"],
      metaLeft: "egress",
    },
  },
];

export const defaultEdges: Edge[] = [];
