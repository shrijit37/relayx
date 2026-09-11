import type { Edge } from "@xyflow/react";
import type { RelayNode } from "./nodes";

export const initialNodes: RelayNode[] = [
  {
    id: "input",
    type: "relay",
    position: { x: 0, y: 180 },
    data: {
      kind: "input",
      title: "HTTP Request",
      lines: ["OpenAI Responses", "POST /v1/responses"],
      metaLeft: "ingress",
      status: "healthy",
    },
  },
  {
    id: "policy",
    type: "relay",
    position: { x: 280, y: 40 },
    data: {
      kind: "policy",
      title: "production-standard",
      lines: ["9 compiled rules", "deterministic matcher"],
      metaLeft: "0.1 ms",
      status: "healthy",
    },
  },
  {
    id: "route",
    type: "relay",
    position: { x: 280, y: 180 },
    data: {
      kind: "route",
      title: "Model Router",
      lines: ["match: claude-*", "strategy: latency"],
      metaLeft: "4 candidates",
      status: "healthy",
    },
  },
  {
    id: "mcp",
    type: "relay",
    position: { x: 280, y: 340 },
    data: {
      kind: "mcp",
      title: "github pull request",
      lines: ["12 candidates → 3 permitted", "1 activated"],
      badges: ["cache HIT", "1.8 ms"],
      metaLeft: "progressive",
      issue: "error",
      status: "degraded",
    },
  },
  {
    id: "skill",
    type: "relay",
    position: { x: 280, y: 500 },
    data: {
      kind: "skill",
      title: "code-review",
      lines: ["metadata → instructions", "references deferred"],
      badges: ["v1.4.0", "18 KB loaded"],
      metaLeft: "2 references",
      status: "ready",
    },
  },
  {
    id: "lane-us",
    type: "relay",
    position: { x: 580, y: 120 },
    data: {
      kind: "lane",
      title: "anthropic-us-vpn",
      lines: ["WireGuard US-01", "pool: warm · reuse 94.2%"],
      badges: ["production-standard"],
      metaLeft: "82 ms",
      status: "healthy",
    },
  },
  {
    id: "lane-eu",
    type: "relay",
    position: { x: 580, y: 300 },
    data: {
      kind: "lane",
      title: "anthropic-eu-vpn",
      lines: ["WireGuard EU-03", "pool: warm · reuse 91.0%"],
      badges: ["eu-residency"],
      metaLeft: "119 ms",
      status: "healthy",
    },
  },
  {
    id: "transform",
    type: "relay",
    position: { x: 860, y: 120 },
    data: {
      kind: "transform",
      title: "OpenAI → Anthropic",
      lines: ["protocol translation", "tools + reasoning preserved"],
      metaLeft: "1.2 ms",
      status: "healthy",
    },
  },
  {
    id: "retry",
    type: "relay",
    position: { x: 860, y: 300 },
    data: {
      kind: "retry",
      title: "2 attempts",
      lines: ["exponential backoff", "budget: 900 ms"],
      metaLeft: "idempotent",
      status: "healthy",
    },
  },
  {
    id: "provider",
    type: "relay",
    position: { x: 1140, y: 120 },
    data: {
      kind: "provider",
      title: "Anthropic · Claude Sonnet",
      lines: ["api.anthropic.com", "messages/stream"],
      badges: ["Streaming", "Tools", "Reasoning", "Structured"],
      metaLeft: "TTFB 420 ms",
      status: "healthy",
    },
  },
  {
    id: "fallback",
    type: "relay",
    position: { x: 1140, y: 320 },
    data: {
      kind: "fallback",
      title: "Anthropic → OpenAI",
      lines: ["on: 429, 5xx, network", "degrade: no deferred tools"],
      metaLeft: "0 triggers / 1h",
      issue: "warn",
      status: "warning",
    },
  },
  {
    id: "condition",
    type: "relay",
    position: { x: 1420, y: 180 },
    data: {
      kind: "condition",
      title: "latency < 150 ms",
      lines: ["source: lane.p95"],
      branches: ["true", "false"],
      metaLeft: "gateway eval",
    },
  },
  {
    id: "output",
    type: "relay",
    position: { x: 1700, y: 110 },
    data: {
      kind: "output",
      title: "Streaming Response",
      lines: ["SSE · text/event-stream", "backpressure aware"],
      metaLeft: "138 open",
      status: "healthy",
    },
  },
  {
    id: "trace",
    type: "relay",
    position: { x: 1700, y: 300 },
    data: {
      kind: "observability",
      title: "Trace",
      lines: ["OpenTelemetry OTLP", "no prompt capture"],
      metaLeft: "sample 100%",
      status: "healthy",
    },
  },
];

export const initialEdges: Edge[] = [
  { id: "e1", source: "input", target: "route" },
  { id: "e2", source: "input", target: "policy" },
  { id: "e3", source: "input", target: "mcp" },
  { id: "e4", source: "mcp", target: "skill" },
  { id: "e5", source: "route", target: "lane-us" },
  { id: "e6", source: "route", target: "lane-eu" },
  { id: "e7", source: "policy", target: "lane-us" },
  { id: "e8", source: "skill", target: "lane-eu" },
  { id: "e9", source: "lane-us", target: "transform" },
  { id: "e10", source: "lane-eu", target: "retry", className: "edge-error" },
  { id: "e11", source: "transform", target: "provider" },
  { id: "e12", source: "retry", target: "provider" },
  { id: "e13", source: "provider", target: "condition" },
  { id: "e14", source: "provider", target: "fallback" },
  { id: "e15", source: "fallback", target: "condition" },
  { id: "e16", source: "condition", sourceHandle: "true", target: "output", label: "true" },
  { id: "e17", source: "condition", sourceHandle: "false", target: "trace", label: "false" },
  { id: "e18", source: "output", target: "trace" },
];

/** Nodes on the hot execution path, in order — used by run/debug mode. */
export const executionPath = [
  { id: "input", label: "Input", ms: "0.2 ms" },
  { id: "route", label: "Route", ms: "0.1 ms" },
  { id: "lane-us", label: "Lane", ms: "0.3 ms" },
  { id: "transform", label: "Protocol translation", ms: "1.2 ms" },
  { id: "provider", label: "Anthropic", ms: "420 ms TTFB" },
  { id: "output", label: "Streaming", ms: "1.39 s" },
];

export const executionEdges = ["e1", "e5", "e9", "e11", "e13", "e16"];
