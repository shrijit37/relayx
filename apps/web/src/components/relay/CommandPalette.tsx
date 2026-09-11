import { useNavigate } from "@tanstack/react-router";
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from "@/components/ui/command";

type Cmd = { label: string; to?: string; shortcut?: string; group: string };

const commands: Cmd[] = [
  { label: "Create workflow", to: "/workflows", shortcut: "⌘N", group: "Workflow" },
  { label: "Validate workflow", to: "/workflows/production-gateway", shortcut: "⌘⇧V", group: "Workflow" },
  { label: "Publish workflow", to: "/workflows/production-gateway/versions", shortcut: "⌘⇧P", group: "Workflow" },
  { label: "Run workflow (test)", to: "/workflows/production-gateway", shortcut: "⌘↵", group: "Workflow" },
  { label: "Open version history", to: "/workflows/production-gateway/versions", group: "Workflow" },
  { label: "Add provider", to: "/providers", group: "Infrastructure" },
  { label: "Add lane", to: "/lanes", group: "Infrastructure" },
  { label: "Add MCP discovery", to: "/mcp", group: "Infrastructure" },
  { label: "Search tools", to: "/mcp", group: "Infrastructure" },
  { label: "Open observability", to: "/observability", shortcut: "⌘⇧O", group: "Telemetry" },
  { label: "Open runs", to: "/runs", group: "Telemetry" },
  { label: "Open latest run", to: "/runs/8F31A2", group: "Telemetry" },
  { label: "Open policies", to: "/policies", group: "Governance" },
  { label: "Open secrets", to: "/secrets", group: "Governance" },
  { label: "System health", to: "/health", group: "Governance" },
];

export function CommandPalette({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
}) {
  const navigate = useNavigate();
  const groups = Array.from(new Set(commands.map((c) => c.group)));

  return (
    <CommandDialog open={open} onOpenChange={onOpenChange}>
      <CommandInput placeholder="Run a command or jump to a surface..." />
      <CommandList>
        <CommandEmpty>No matching command.</CommandEmpty>
        {groups.map((g) => (
          <CommandGroup key={g} heading={g}>
            {commands
              .filter((c) => c.group === g)
              .map((c) => (
                <CommandItem
                  key={c.label}
                  value={c.label}
                  onSelect={() => {
                    onOpenChange(false);
                    if (c.to) navigate({ to: c.to });
                  }}
                >
                  <span className="text-xs">{c.label}</span>
                  {c.shortcut ? <CommandShortcut className="num">{c.shortcut}</CommandShortcut> : null}
                </CommandItem>
              ))}
          </CommandGroup>
        ))}
      </CommandList>
    </CommandDialog>
  );
}
