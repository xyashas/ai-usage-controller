import type { Dest, DestOption, Tool } from "./types";

// Remembers where you like each tool to open (app, terminal or web). Per
// computer only; the app works the same without it.
const key = (t: Tool) => `aiuc.dest.${t}`;

export function preferredDest(tool: Tool, options: DestOption[]): DestOption | undefined {
  let saved: string | null = null;
  try { saved = localStorage.getItem(key(tool)); } catch { /* storage unavailable */ }
  return options.find((o) => o.dest === saved) ?? options[0];
}

export function rememberDest(tool: Tool, dest: Dest) {
  try { localStorage.setItem(key(tool), dest); } catch { /* storage unavailable */ }
}
