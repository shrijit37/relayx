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
  { label: "Create workflow", to: "/workflows", group: "Workflow" },
  { label: "Open workflows", to: "/workflows", group: "Workflow" },
  { label: "Open providers", to: "/providers", group: "Infrastructure" },
  { label: "Open lanes", to: "/lanes", group: "Infrastructure" },
  { label: "Open system health", to: "/health", group: "System" },
  { label: "Open settings", to: "/settings", group: "System" },
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
