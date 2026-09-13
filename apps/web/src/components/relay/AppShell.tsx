import { useEffect, useState, type ReactNode } from "react";
import { Link, useRouterState } from "@tanstack/react-router";
import {
  Activity,
  Boxes,
  ChevronsLeft,
  ChevronsRight,
  Command,
  Gauge,
  GitBranch,
  Grid2x2,
  Heart,
  KeyRound,
  Layers,
  type LucideIcon,
  Network,
  PlayCircle,
  Search,
  Settings,
  ShieldCheck,
  Sparkles,
  Waypoints,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { useSystemHealth } from "@/lib/use-workflow-publication";
import { CommandPalette } from "./CommandPalette";
import { StatusDot } from "./primitives";

type NavItem = { to: string; label: string; icon: LucideIcon };

const workspaceNav: NavItem[] = [
  { to: "/", label: "Overview", icon: Grid2x2 },
  { to: "/workflows", label: "Workflows", icon: GitBranch },
  { to: "/runs", label: "Runs", icon: PlayCircle },
  { to: "/providers", label: "Providers", icon: Boxes },
  { to: "/lanes", label: "Lanes", icon: Network },
  { to: "/mcp", label: "MCP / Tools", icon: Waypoints },
  { to: "/skills", label: "Skills", icon: Sparkles },
  { to: "/policies", label: "Policies", icon: ShieldCheck },
  { to: "/secrets", label: "Secrets", icon: KeyRound },
  { to: "/observability", label: "Observability", icon: Activity },
];

const systemNav: NavItem[] = [
  { to: "/health", label: "Health", icon: Heart },
  { to: "/settings", label: "Settings", icon: Settings },
];

export function AppShell({ children, flush = false }: { children: ReactNode; flush?: boolean }) {
  const [collapsed, setCollapsed] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const { data: health } = useSystemHealth();

  // Real gateway readiness, feigned nowhere: the dot reflects the live probe
  // (or "unknown" when the control plane is unreachable).
  const healthStatus = health
    ? health.gateway.ready.status === "ready" && health.gateway.healthz.status === "ok"
      ? "healthy"
      : "degraded"
    : "unknown";
  const healthLabel =
    health?.gateway.ready.status === "ready" && health?.gateway.healthz.status === "ok"
      ? "Gateway ready"
      : health
        ? "Gateway degraded"
        : "Gateway unknown";
  const healthGatewayLine = health
    ? `gateway /healthz=${health.gateway.healthz.status} /ready=${health.gateway.ready.status}`
    : "control plane unreachable";

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() === "k" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        setPaletteOpen((o) => !o);
      }
      if (e.key === "[" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        setCollapsed((c) => !c);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const isActive = (to: string) => (to === "/" ? pathname === "/" : pathname.startsWith(to));

    return (
         <div className="flex h-screen w-full overflow-hidden bg-background">
      <aside
        className={cn(
          "hidden shrink-0 flex-col border-r border-sidebar-border bg-sidebar md:flex",
          collapsed ? "w-[52px]" : "w-[212px]",
        )}
      >
        <div className="flex h-11 items-center gap-2 border-b border-sidebar-border px-3">
          <span className="grid size-5 shrink-0 place-items-center overflow-hidden rounded-[4px] bg-black">
            <img src="/logo.svg" alt="relay-x" className="size-full" />
          </span>
          {!collapsed && (
            <span className="text-[13px] font-semibold tracking-tight">
              relay<span className="text-primary">-x</span>
            </span>
          )}
        </div>

        <button
          onClick={() => setPaletteOpen(true)}
          className="focus-ring mx-2 mt-2 flex h-7 items-center gap-2 rounded-sm border border-sidebar-border bg-background px-2 text-left text-xs text-muted-foreground transition-colors hover:border-border-strong"
        >
          <Search className="size-3.5 shrink-0" />
          {!collapsed && (
            <>
              <span className="flex-1 truncate">Search</span>
              <kbd className="num rounded-[3px] border border-border px-1 text-[10px]">⌘K</kbd>
            </>
          )}
        </button>

        <nav className="mt-3 min-h-0 flex-1 overflow-y-auto px-2 pb-2">
          {!collapsed && <div className="label-xs px-1.5 pb-1">Workspace</div>}
          <ul className="space-y-px">
            {workspaceNav.map((item) => (
              <NavRow key={item.to} item={item} collapsed={collapsed} active={isActive(item.to)} />
            ))}
          </ul>
          {!collapsed && <div className="label-xs px-1.5 pt-4 pb-1">System</div>}
          <ul className={cn("space-y-px", collapsed && "mt-3 border-t border-sidebar-border pt-3")}>
            {systemNav.map((item) => (
              <NavRow key={item.to} item={item} collapsed={collapsed} active={isActive(item.to)} />
            ))}
          </ul>
        </nav>

        <div className="border-t border-sidebar-border p-2">
          {!collapsed ? (
            <div className="space-y-1.5">
              <button className="focus-ring flex h-7 w-full items-center gap-2 rounded-sm px-1.5 text-xs hover:bg-sidebar-accent">
                <Layers className="size-3.5 text-muted-foreground" />
                <span className="truncate">local development</span>
              </button>
              <button
                title={healthGatewayLine}
                className="focus-ring flex h-7 w-full items-center gap-2 rounded-sm border border-sidebar-border px-1.5 text-xs hover:bg-sidebar-accent"
              >
                <StatusDot status={healthStatus} />
                <span className="truncate">{healthLabel}</span>
                <Gauge className="ml-auto size-3.5 text-muted-foreground" />
              </button>
              <button className="focus-ring flex h-8 w-full items-center gap-2 rounded-sm px-1.5 hover:bg-sidebar-accent">
                <span className="grid size-5 place-items-center rounded-full bg-panel-raised text-[9px] font-semibold">
                  {"dev"}
                </span>
                <span className="min-w-0 flex-1 truncate text-left text-xs">local user</span>
              </button>
            </div>
          ) : (
            <div className="flex flex-col items-center gap-2">
              <StatusDot status={healthStatus} />
              <span className="grid size-6 place-items-center rounded-full bg-panel-raised text-[9px] font-semibold">
                {"dv"}
              </span>
            </div>
          )}
          <button
            onClick={() => setCollapsed((c) => !c)}
            className="focus-ring mt-2 flex h-6 w-full items-center justify-center rounded-sm text-muted-foreground hover:bg-sidebar-accent"
            aria-label="Toggle sidebar"
          >
            {collapsed ? <ChevronsRight className="size-3.5" /> : <ChevronsLeft className="size-3.5" />}
          </button>
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex h-11 shrink-0 items-center gap-2 border-b border-border bg-panel px-3 md:hidden">
          <span className="grid size-5 shrink-0 place-items-center overflow-hidden rounded-[4px] bg-black">
            <img src="/logo.svg" alt="relay-x" className="size-full" />
          </span>
          <span className="text-[13px] font-semibold">relay-x</span>
          <button
            onClick={() => setPaletteOpen(true)}
            className="focus-ring ml-auto flex h-7 items-center gap-1.5 rounded-sm border border-border px-2 text-xs text-muted-foreground"
          >
            <Command className="size-3.5" /> Menu
          </button>
        </div>
        <main className={cn("min-h-0 flex-1", flush ? "overflow-hidden" : "overflow-y-auto")}>{children}</main>
      </div>

      <CommandPalette open={paletteOpen} onOpenChange={setPaletteOpen} />
    </div>
  );
}

function NavRow({ item, collapsed, active }: { item: NavItem; collapsed: boolean; active: boolean }) {
  const Icon = item.icon;
  return (
    <li>
      <Link
        to={item.to}
        title={item.label}
        className={cn(
          "focus-ring flex h-7 items-center gap-2 rounded-sm px-1.5 text-xs transition-colors",
          collapsed && "justify-center px-0",
          active
            ? "bg-sidebar-accent font-medium text-sidebar-accent-foreground"
            : "text-sidebar-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground",
        )}
      >
        <Icon className={cn("size-3.5 shrink-0", active ? "text-primary" : "text-muted-foreground")} />
        {!collapsed && <span className="truncate">{item.label}</span>}
      </Link>
    </li>
  );
}
