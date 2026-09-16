/**
 * VersionCompare — renders a two-column structural diff between two versions.
 *
 * Shows added (green), removed (red), and changed (yellow) nodes/edges.
 * For changed nodes, individual config fields are listed.
 */

import type { VersionRow } from "@/lib/api";
import { compareVersions, type DiffEntry } from "@/lib/version-compare";
import type { WorkflowJson } from "@/lib/workflow";
import { cn } from "@/lib/utils";

function DiffRow({ entry }: { entry: DiffEntry }) {
  const colorMap = {
    added: "border-ok/40 bg-ok/8",
    removed: "border-fail/40 bg-fail/8",
    changed: "border-warn/40 bg-warn/8",
  };
  const badgeColor = {
    added: "bg-ok/20 text-ok",
    removed: "bg-fail/20 text-fail",
    changed: "bg-warn/20 text-warn",
  };
  const label =
    entry.type === "node"
      ? `${entry.nodeId} (${entry.schemaKind})`
      : entry.edgeLabel;

  return (
    <div className={cn("rounded-sm border px-2.5 py-1.5", colorMap[entry.kind])}>
      <div className="flex items-center gap-2">
        <span
          className={cn(
            "rounded-sm px-1 py-0.5 text-[9px] font-medium uppercase",
            badgeColor[entry.kind],
          )}
        >
          {entry.kind}
        </span>
        <span className="num text-xs">{label}</span>
        <span className="text-[10px] text-muted-foreground">{entry.type}</span>
      </div>
      {entry.fields && entry.fields.length > 0 && (
        <div className="mt-1 space-y-0.5 pl-4">
          {entry.fields.map((f, i) => (
            <div key={i} className="flex items-start gap-2 text-[10px]">
              <span className="num min-w-[100px] shrink-0 text-muted-foreground">{f.field}</span>
              <span className="num shrink-0 text-fail line-through opacity-60">
                {formatValue(f.oldValue)}
              </span>
              <span className="num text-ok">{formatValue(f.newValue)}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function formatValue(v: unknown): string {
  if (v === null || v === undefined) return "—";
  if (typeof v === "object") return JSON.stringify(v);
  return String(v);
}

function summaryLabel(result: ReturnType<typeof compareVersions>): string {
  if (result.addedCount === 0 && result.removedCount === 0 && result.changedCount === 0) {
    return "no differences — versions are identical";
  }
  const parts: string[] = [];
  if (result.addedCount > 0) parts.push(`${result.addedCount} added`);
  if (result.removedCount > 0) parts.push(`${result.removedCount} removed`);
  if (result.changedCount > 0) parts.push(`${result.changedCount} changed`);
  return parts.join(", ");
}

export function VersionCompare({
  oldVersion,
  newVersion,
}: {
  oldVersion: VersionRow;
  newVersion: VersionRow;
}) {
  const result = compareVersions(
    oldVersion.workflow_json as WorkflowJson,
    newVersion.workflow_json as WorkflowJson,
  );

  const allEntries: DiffEntry[] = [...result.nodes, ...result.edges];

  return (
    <div className="space-y-3 p-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3 text-xs">
          <span className="font-medium">
            v{oldVersion.version}{" "}
            <span className="text-muted-foreground">
              ({oldVersion.status})
            </span>
          </span>
          <span className="text-muted-foreground">vs</span>
          <span className="font-medium">
            v{newVersion.version}{" "}
            <span className="text-muted-foreground">
              ({newVersion.status})
            </span>
          </span>
        </div>
        <span className="num text-[11px] text-muted-foreground">
          {summaryLabel(result)}
        </span>
      </div>

      {allEntries.length === 0 ? (
        <div className="rounded-sm border border-border bg-panel px-4 py-8 text-center text-xs text-muted-foreground">
          Both versions have identical workflow content.
        </div>
      ) : (
        <div className="space-y-1.5">
          {allEntries.map((entry, i) => (
            <DiffRow key={i} entry={entry} />
          ))}
        </div>
      )}
    </div>
  );
}
