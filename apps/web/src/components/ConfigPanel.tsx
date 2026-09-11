// Configuration panel — renders node-specific config when selected

import { useWorkflowStore } from "../store/workflow-store";
import {
  type WorkflowNode,
  NodeKind,
  RouterStrategy,
  TransformOperation,
  ConditionOp,
  PortType,
} from "../types/workflow";

export function ConfigPanel() {
  const selectedNodeId = useWorkflowStore((s) => s.selectedNodeId);
  const nodes = useWorkflowStore((s) => s.nodes);
  const updateNodeConfig = useWorkflowStore((s) => s.updateNodeConfig);

  const selectedNode = nodes.find((n) => n.id === selectedNodeId);

  if (!selectedNode) {
    return (
      <div className="config-panel">
        <h3>Configuration</h3>
        <p className="config-empty">Select a node to configure</p>
      </div>
    );
  }

  const node = selectedNode.data.node as WorkflowNode;

  return (
    <div className="config-panel">
      <h3>{node.kind} Configuration</h3>
      <div className="config-fields">
        {node.kind === NodeKind.Llm && (
          <LlmFields node={node} update={updateNodeConfig} />
        )}
        {node.kind === NodeKind.Mcp && (
          <McpFields node={node} update={updateNodeConfig} />
        )}
        {node.kind === NodeKind.Skill && (
          <SkillFields node={node} update={updateNodeConfig} />
        )}
        {node.kind === NodeKind.Condition && (
          <ConditionFields node={node} update={updateNodeConfig} />
        )}
        {node.kind === NodeKind.Router && (
          <RouterFields node={node} update={updateNodeConfig} />
        )}
        {node.kind === NodeKind.Transform && (
          <TransformFields node={node} update={updateNodeConfig} />
        )}
        {node.kind === NodeKind.Input && (
          <InputFields node={node} update={updateNodeConfig} />
        )}
        {node.kind === NodeKind.Output && (
          <OutputFields node={node} update={updateNodeConfig} />
        )}
      </div>
    </div>
  );
}

function LlmFields({
  node,
  update,
}: {
  node: WorkflowNode;
  update: (id: string, config: Record<string, unknown>) => void;
}) {
  const cfg = node.config as {
    protocol?: string;
    model?: string;
    temperature?: number;
    max_tokens?: number;
    stream: boolean;
    lane_id?: string;
  };

  return (
    <>
      <label>
        Model
        <input
          type="text"
          value={cfg.model ?? ""}
          onChange={(e) => update(node.id, { model: e.target.value })}
          placeholder="e.g. gpt-4o, claude-sonnet-4-20250514"
        />
      </label>
      <label>
        Protocol
        <select
          value={cfg.protocol ?? ""}
          onChange={(e) => update(node.id, { protocol: e.target.value || undefined })}
        >
          <option value="">Auto</option>
          <option value="OpenAiChatCompletions">OpenAI Chat</option>
          <option value="AnthropicMessages">Anthropic Messages</option>
          <option value="OpenAiResponses">OpenAI Responses</option>
        </select>
      </label>
      <label>
        Lane
        <input
          type="text"
          value={cfg.lane_id ?? ""}
          onChange={(e) => update(node.id, { lane_id: e.target.value || undefined })}
          placeholder="Optional lane ID"
        />
      </label>
      <label>
        Temperature
        <input
          type="number"
          min={0}
          max={2}
          step={0.1}
          value={cfg.temperature ?? 0.7}
          onChange={(e) =>
            update(node.id, { temperature: parseFloat(e.target.value) })
          }
        />
      </label>
      <label>
        Max Tokens
        <input
          type="number"
          min={1}
          value={cfg.max_tokens ?? 4096}
          onChange={(e) =>
            update(node.id, { max_tokens: parseInt(e.target.value) })
          }
        />
      </label>
      <label className="checkbox-label">
        <input
          type="checkbox"
          checked={cfg.stream}
          onChange={(e) => update(node.id, { stream: e.target.checked })}
        />
        Stream response
      </label>
    </>
  );
}

function McpFields({
  node,
  update,
}: {
  node: WorkflowNode;
  update: (id: string, config: Record<string, unknown>) => void;
}) {
  const cfg = node.config as {
    server_ref: string;
    tool_name: string;
    deferred: boolean;
  };

  return (
    <>
      <label>
        Server Reference
        <input
          type="text"
          value={cfg.server_ref}
          onChange={(e) => update(node.id, { server_ref: e.target.value })}
          placeholder="MCP server identifier"
        />
      </label>
      <label>
        Tool Name
        <input
          type="text"
          value={cfg.tool_name}
          onChange={(e) => update(node.id, { tool_name: e.target.value })}
          placeholder="Tool to invoke"
        />
      </label>
      <label className="checkbox-label">
        <input
          type="checkbox"
          checked={cfg.deferred}
          onChange={(e) => update(node.id, { deferred: e.target.checked })}
        />
        Deferred discovery
      </label>
    </>
  );
}

function SkillFields({
  node,
  update,
}: {
  node: WorkflowNode;
  update: (id: string, config: Record<string, unknown>) => void;
}) {
  const cfg = node.config as {
    skill_ref: string;
    progressive: boolean;
  };

  return (
    <>
      <label>
        Skill Reference
        <input
          type="text"
          value={cfg.skill_ref}
          onChange={(e) => update(node.id, { skill_ref: e.target.value })}
          placeholder="Skill identifier"
        />
      </label>
      <label className="checkbox-label">
        <input
          type="checkbox"
          checked={cfg.progressive}
          onChange={(e) => update(node.id, { progressive: e.target.checked })}
        />
        Progressive loading
      </label>
    </>
  );
}

function ConditionFields({
  node,
  update,
}: {
  node: WorkflowNode;
  update: (id: string, config: Record<string, unknown>) => void;
}) {
  const cfg = node.config as {
    field: string;
    operator: ConditionOp;
    value: string;
  };

  return (
    <>
      <label>
        Field
        <input
          type="text"
          value={cfg.field}
          onChange={(e) => update(node.id, { field: e.target.value })}
          placeholder="e.g. llm.response.score"
        />
      </label>
      <label>
        Operator
        <select
          value={cfg.operator}
          onChange={(e) =>
            update(node.id, { operator: e.target.value as ConditionOp })
          }
        >
          {Object.values(ConditionOp).map((op) => (
            <option key={op} value={op}>
              {op}
            </option>
          ))}
        </select>
      </label>
      <label>
        Value
        <input
          type="text"
          value={cfg.value}
          onChange={(e) => update(node.id, { value: e.target.value })}
          placeholder="Comparison value"
        />
      </label>
    </>
  );
}

function RouterFields({
  node,
  update,
}: {
  node: WorkflowNode;
  update: (id: string, config: Record<string, unknown>) => void;
}) {
  const cfg = node.config as { strategy: RouterStrategy };

  return (
    <label>
      Strategy
      <select
        value={cfg.strategy}
        onChange={(e) =>
          update(node.id, { strategy: e.target.value as RouterStrategy })
        }
      >
        {Object.values(RouterStrategy).map((s) => (
          <option key={s} value={s}>
            {s}
          </option>
        ))}
      </select>
    </label>
  );
}

function TransformFields({
  node,
  update,
}: {
  node: WorkflowNode;
  update: (id: string, config: Record<string, unknown>) => void;
}) {
  const cfg = node.config as { operation: TransformOperation };

  return (
    <label>
      Operation
      <select
        value={cfg.operation}
        onChange={(e) =>
          update(node.id, {
            operation: e.target.value as TransformOperation,
          })
        }
      >
        {Object.values(TransformOperation).map((op) => (
          <option key={op} value={op}>
            {op}
          </option>
        ))}
      </select>
    </label>
  );
}

function InputFields({
  node,
  update,
}: {
  node: WorkflowNode;
  update: (id: string, config: Record<string, unknown>) => void;
}) {
  const cfg = node.config as { input_type: PortType };

  return (
    <label>
      Input Type
      <select
        value={cfg.input_type}
        onChange={(e) =>
          update(node.id, { input_type: e.target.value as PortType })
        }
      >
        {Object.values(PortType).map((t) => (
          <option key={t} value={t}>
            {t}
          </option>
        ))}
      </select>
    </label>
  );
}

function OutputFields({
  node,
  update,
}: {
  node: WorkflowNode;
  update: (id: string, config: Record<string, unknown>) => void;
}) {
  const cfg = node.config as { output_type: PortType };

  return (
    <label>
      Output Type
      <select
        value={cfg.output_type}
        onChange={(e) =>
          update(node.id, { output_type: e.target.value as PortType })
        }
      >
        {Object.values(PortType).map((t) => (
          <option key={t} value={t}>
            {t}
          </option>
        ))}
      </select>
    </label>
  );
}