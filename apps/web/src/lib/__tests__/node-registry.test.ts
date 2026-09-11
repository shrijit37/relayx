import { describe, it, expect } from "vitest";
import { NodeKind } from "../../types/workflow";
import {
  NODE_REGISTRY,
  getNodeTypeInfo,
  canConnect,
} from "../node-registry";

describe("node-registry", () => {
  it("has all 8 node kinds", () => {
    expect(NODE_REGISTRY).toHaveLength(8);
    for (const kind of Object.values(NodeKind)) {
      expect(NODE_REGISTRY.some((n) => n.kind === kind)).toBe(true);
    }
  });

  it("getNodeTypeInfo returns info for every kind", () => {
    for (const kind of Object.values(NodeKind)) {
      const info = getNodeTypeInfo(kind);
      expect(info.kind).toBe(kind);
      expect(info.label).toBeTruthy();
      expect(info.icon).toBeTruthy();
    }
  });

  it("canConnect accepts compatible types", () => {
    expect(canConnect("Message", "Message")).toBe(true);
    expect(canConnect("Message", "Stream")).toBe(true);
    expect(canConnect("ToolCall", "ToolResult")).toBe(true);
    expect(canConnect("ToolResult", "ToolCall")).toBe(true);
  });

  it("canConnect rejects incompatible types", () => {
    expect(canConnect("Message", "Bool")).toBe(false);
    expect(canConnect("Bool", "Message")).toBe(false);
    expect(canConnect("ToolCall", "Message")).toBe(false);
  });
});