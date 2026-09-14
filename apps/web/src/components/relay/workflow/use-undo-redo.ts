/**
 * Snapshot-based undo/redo for nodes + edges.
 * Call `record()` BEFORE a structural mutation (drop, connect, delete, drag
 * start) so the pre-change state is captured via React Flow's live getters;
 * `undo()`/`redo()` restore through the state setters.
 */

import { useCallback, useRef, useState } from "react";
import type { Edge, Node } from "@xyflow/react";

type Snapshot<N extends Node> = { nodes: N[]; edges: Edge[] };

export function useUndoRedo<N extends Node>(opts: {
  getNodes: () => N[];
  getEdges: () => Edge[];
  setNodes: (updater: (ns: N[]) => N[]) => void;
  setEdges: (updater: (es: Edge[]) => Edge[]) => void;
}) {
  const { getNodes, getEdges, setNodes, setEdges } = opts;
  const past = useRef<Snapshot<N>[]>([]);
  const future = useRef<Snapshot<N>[]>([]);
  const [canUndo, setCanUndo] = useState(false);
  const [canRedo, setCanRedo] = useState(false);

  const current = useCallback((): Snapshot<N> => ({ nodes: getNodes(), edges: getEdges() }), [getNodes, getEdges]);

  const record = useCallback(() => {
    past.current.push(current());
    if (past.current.length > 100) past.current.shift();
    future.current = [];
    setCanUndo(true);
    setCanRedo(false);
  }, [current]);

  const apply = useCallback((snap: Snapshot<N>) => {
    setNodes(() => snap.nodes);
    setEdges(() => snap.edges);
  }, [setNodes, setEdges]);

  const undo = useCallback(() => {
    if (!past.current.length) return false;
    future.current.push(current());
    apply(past.current.pop()!);
    setCanUndo(past.current.length > 0);
    setCanRedo(true);
    return true;
  }, [current, apply]);

  const redo = useCallback(() => {
    if (!future.current.length) return false;
    past.current.push(current());
    apply(future.current.pop()!);
    setCanUndo(true);
    setCanRedo(future.current.length > 0);
    return true;
  }, [current, apply]);

  return { record, undo, redo, canUndo, canRedo };
}
