import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

type Tone = "ok" | "warn" | "fail" | "info" | "violet" | "neutral";

const toneText: Record<Tone, string> = {
  ok: "text-ok",
  warn: "text-warn",
  fail: "text-fail",
  info: "text-info",
  violet: "text-violet",
  neutral: "text-muted-foreground",
};

const toneBg: Record<Tone, string> = {
  ok: "bg-ok",
  warn: "bg-warn",
  fail: "bg-fail",
  info: "bg-info",
  violet: "bg-violet",
  neutral: "bg-muted-foreground",
};

export function statusTone(status: string): Tone {
  const s = status.toLowerCase();
  if (["healthy", "completed", "ready", "valid", "deployed", "production", "allow", "hit", "ok", "success"].includes(s))
    return "ok";
  if (["degraded", "warning", "warn", "draft", "queued", "pending", "metadata only"].includes(s)) return "warn";
  if (["failed", "failure", "error", "invalid", "deny", "unauthorized"].includes(s)) return "fail";
  if (["running", "streaming", "active", "selected"].includes(s)) return "info";
  return "neutral";
}

export function StatusDot({ status, className }: { status: string; className?: string }) {
  const tone = statusTone(status);
  return (
    <span
      className={cn(
        "inline-block size-[7px] shrink-0 rounded-full",
        toneBg[tone],
        tone !== "neutral" && "shadow-[0_0_0_3px_color-mix(in_oklab,currentColor_18%,transparent)]",
        toneText[tone],
        className,
      )}
    />
  );
}

export function StatusText({ status, className }: { status: string; className?: string }) {
  return (
    <span className={cn("inline-flex items-center gap-1.5 text-xs", toneText[statusTone(status)], className)}>
      <StatusDot status={status} />
      {status}
    </span>
  );
}

export function Tag({
  children,
  tone = "neutral",
  className,
}: {
  children: ReactNode;
  tone?: Tone;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded-sm border px-1.5 py-0.5 text-[10px] leading-4 font-medium",
        tone === "neutral"
          ? "border-border bg-muted text-muted-foreground"
          : cn("border-current/25 bg-current/10", toneText[tone]),
        className,
      )}
    >
      {children}
    </span>
  );
}

export function SectionLabel({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn("label-xs", className)}>{children}</div>;
}

export function Panel({
  title,
  actions,
  children,
  className,
  bodyClassName,
  dense,
}: {
  title?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
  bodyClassName?: string;
  dense?: boolean;
}) {
  return (
    <section className={cn("panel-surface flex flex-col overflow-hidden", className)}>
      {title ? (
        <header className="hairline-b flex h-9 shrink-0 items-center justify-between gap-3 px-3">
          <div className="label-xs">{title}</div>
          <div className="flex items-center gap-1.5">{actions}</div>
        </header>
      ) : null}
      <div className={cn(dense ? "" : "p-3", "min-h-0 flex-1", bodyClassName)}>{children}</div>
    </section>
  );
}

export function KV({
  k,
  v,
  mono = true,
  tone,
}: {
  k: string;
  v: ReactNode;
  mono?: boolean;
  tone?: Tone;
}) {
  return (
    <div className="flex items-baseline justify-between gap-4 py-[5px]">
      <span className="text-xs text-muted-foreground">{k}</span>
      <span className={cn("text-xs", mono && "num", tone && toneText[tone])}>{v}</span>
    </div>
  );
}

export function Metric({
  label,
  value,
  unit,
  delta,
  tone = "neutral",
  hint,
}: {
  label: string;
  value: string;
  unit?: string;
  delta?: string;
  tone?: Tone;
  hint?: string;
}) {
  return (
    <div className="panel-surface px-3 py-2.5">
      <div className="label-xs truncate">{label}</div>
      <div className="mt-1.5 flex items-baseline gap-1">
        <span className="num text-xl leading-none font-medium">{value}</span>
        {unit ? <span className="num text-xs text-muted-foreground">{unit}</span> : null}
      </div>
      <div className="mt-1.5 flex items-center justify-between gap-2">
        <span className="truncate text-[10px] text-muted-foreground">{hint}</span>
        {delta ? <span className={cn("num text-[10px]", toneText[tone])}>{delta}</span> : null}
      </div>
    </div>
  );
}

export function PageHeader({
  title,
  subtitle,
  actions,
  meta,
}: {
  title: string;
  subtitle?: string;
  actions?: ReactNode;
  meta?: ReactNode;
}) {
  return (
    <div className="hairline-b flex flex-wrap items-end justify-between gap-3 px-4 py-3">
      <div className="min-w-0">
        <h1 className="text-sm font-semibold tracking-tight">{title}</h1>
        {subtitle ? <p className="mt-0.5 text-xs text-muted-foreground">{subtitle}</p> : null}
        {meta ? <div className="mt-2 flex flex-wrap items-center gap-3">{meta}</div> : null}
      </div>
      <div className="flex items-center gap-1.5">{actions}</div>
    </div>
  );
}

export function EmptyState({
  title,
  body,
  action,
}: {
  title: string;
  body: string;
  action?: ReactNode;
}) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 rounded-md border border-dashed border-border px-6 py-14 text-center">
      <div className="text-sm font-medium">{title}</div>
      <p className="max-w-sm text-xs leading-relaxed whitespace-pre-line text-muted-foreground">{body}</p>
      {action ? <div className="mt-2">{action}</div> : null}
    </div>
  );
}

export function Bar({ value, tone = "info" }: { value: number; tone?: Tone }) {
  return (
    <div className="h-1.5 w-full overflow-hidden rounded-full bg-muted">
      <div
        className={cn("h-full rounded-full", toneBg[tone])}
        style={{ width: `${Math.max(1, Math.min(100, value))}%` }}
      />
    </div>
  );
}

export function TableShell({ head, children }: { head: string[]; children: ReactNode }) {
  return (
    <div className="w-full overflow-x-auto">
      <table className="w-full min-w-[720px] border-collapse text-xs">
        <thead>
          <tr className="hairline-b">
            {head.map((h) => (
              <th key={h} className="label-xs px-3 py-2 text-left font-medium whitespace-nowrap">
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>{children}</tbody>
      </table>
    </div>
  );
}

export function Td({ children, className }: { children: ReactNode; className?: string }) {
  return <td className={cn("border-b border-border/60 px-3 py-2 align-middle", className)}>{children}</td>;
}
