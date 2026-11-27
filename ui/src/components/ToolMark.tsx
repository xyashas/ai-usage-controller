import type { Tool } from "../types";

const paths: Record<Tool, string> = {
  claude_code: "M3 11V6l5-3v5Zm5 2V8l5-3v5Z",
  codex: "m3 4 4 4-4 4m6 0h4",
  gemini: "m8 2 5 6-5 6-5-6Zm-5 6h10",
};

export function ToolMark({ tool }: { tool: Tool }) {
  return <svg aria-hidden="true" width="16" height="16" viewBox="0 0 16 16" className="shrink-0" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"><path d={paths[tool]} /></svg>;
}
