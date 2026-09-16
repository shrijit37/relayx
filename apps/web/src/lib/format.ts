/** Human-readable duration between two ISO timestamps (e.g. "1m 24s"). */
export function duration(start: string, end: string): string {
  const ms = Math.abs(new Date(end).getTime() - new Date(start).getTime());
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`;
  return `${Math.floor(ms / 60_000)}m ${Math.round((ms % 60_000) / 1000)}s`;
}
