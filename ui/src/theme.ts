import type { Color, Tool } from "./types";

// Each tool gets a soft tinted tile on the black panel, with dark ink on top.
// The state (fine / getting low / used up) shows in the bar and the number.
export const tint: Record<Tool, { bg: string; ink: string; bar: string }> = {
  claude_code: { bg: "#F1D9CB", ink: "#2A1810", bar: "#C4673F" },
  codex: { bg: "#D3E2F1", ink: "#0F1E2D", bar: "#2F6DAE" },
  gemini: { bg: "#DDD7F3", ink: "#1C1535", bar: "#6A55C4" },
};
export const alertTint = { bg: "#F5D1CC", ink: "#3A100B", bar: "#C8372A" };
export const warnTint = { bg: "#F3E2BC", ink: "#33230A", bar: "#B07A10" };

/** Bar/number colour on a tinted tile: the tool's own colour until it gets low. */
export function stateColor(tool: Tool, color: Color): string {
  if (color === "red") return "#C8372A";
  if (color === "amber") return "#A86F05";
  return tint[tool].bar;
}

export const toolName: Record<Tool, string> = { claude_code: "Claude Code", codex: "Codex", gemini: "Gemini" };
