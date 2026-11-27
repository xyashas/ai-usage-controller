#!/usr/bin/env node
// AI Usage Controller handoff hook.
//
// Claude Code runs this after every turn (a "Stop" hook in .claude/settings.json),
// locally and in the cloud. It rewrites the auto section of .aiuc/handoff.md from
// the session transcript, so the next agent (Codex, Gemini, Claude Code, cloud
// or local) can continue even if this session runs out of usage mid-task.
// It never calls the network and never fails the turn.
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";

const START = "<!-- aiuc:auto -->";
const END = "<!-- /aiuc:auto -->";

// Strip things that look like credentials before anything is written to a
// file that gets committed.
export function redact(s) {
  return s
    .replace(/\b(sk-[A-Za-z0-9_-]{12,}|sk-ant-[A-Za-z0-9_-]{12,}|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|AKIA[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{30,}|xox[abpr]-[A-Za-z0-9-]{10,}|eyJ[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,})/g, "[redacted]")
    .replace(/(https?:\/\/[^\s?#]+)\?[^\s]+/g, "$1?…")
    .replace(/\b((?:password|passwd|secret|token|api[_-]?key)\s*[:=]\s*)\S+/gi, "$1[redacted]");
}

const clip = (s, words = 60) => {
  const w = redact(String(s)).replace(/\s+/g, " ").trim().split(" ");
  const out = w.length > words ? w.slice(0, words).join(" ") + " …" : w.join(" ");
  return out.length > 500 ? out.slice(0, 500) + "…" : out;
};

function textOf(content) {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content.filter((c) => c && c.type === "text" && typeof c.text === "string").map((c) => c.text).join("\n");
}

// Drop blocks the harness wraps around a message (reminders, pasted-content markers).
const clean = (t) => t.replace(/<(system-reminder|local-command-[a-z]+|command-[a-z]+)>[\s\S]*?<\/\1>/g, "").trim();

// Messages the harness inserts (command output, reminders, interrupts) are not requests.
const isNoise = (t) => !t.trim() || /^\s*<[a-z-]+[\s>]/i.test(t) || t.startsWith("[Request interrupted");

export function summarize(lines, root) {
  const s = { goal: "", requests: [], files: [], todos: null, reply: "", errors: [] };
  const push = (list, item, max) => {
    if (!item) return;
    const i = list.indexOf(item);
    if (i >= 0) list.splice(i, 1);
    list.push(item);
    if (list.length > max) list.shift();
  };
  for (const line of lines) {
    let v;
    try { v = JSON.parse(line); } catch { continue; }
    if (!v || v.isSidechain || v.isMeta || v.isCompactSummary) continue;
    const content = v.message?.content;
    if (v.type === "user") {
      const text = clean(textOf(content));
      if (!isNoise(text)) {
        if (!s.goal) s.goal = clip(text, 80);
        else push(s.requests, clip(text), 4);
      }
      if (Array.isArray(content)) {
        for (const c of content) {
          if (c?.type === "tool_result" && c.is_error) push(s.errors, clip(textOf(c.content) || String(c.content ?? ""), 30), 3);
        }
      }
    } else if (v.type === "assistant" && Array.isArray(content)) {
      for (const c of content) {
        if (c?.type === "text" && c.text?.trim()) s.reply = clip(c.text, 90);
        if (c?.type !== "tool_use") continue;
        const input = c.input ?? {};
        if (["Edit", "Write", "MultiEdit", "NotebookEdit"].includes(c.name)) {
          const file = input.file_path || input.notebook_path;
          if (typeof file !== "string") continue;
          const rel = path.relative(root, path.resolve(root, file));
          // Only files inside the project; scratch files elsewhere mean nothing to the next agent.
          if (rel && !rel.startsWith("..") && !path.isAbsolute(rel)) push(s.files, rel.split(path.sep).join("/"), 12);
        }
        if (c.name === "TodoWrite" && Array.isArray(input.todos)) s.todos = input.todos;
      }
    }
  }
  return s;
}

function branch(root) {
  try {
    return execFileSync("git", ["rev-parse", "--abbrev-ref", "HEAD"], { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
  } catch { return ""; }
}

export function render(s, { tool = "Claude Code", where = "local", br = "", now = new Date() } = {}) {
  const out = [START, "## Session log (auto)", ""];
  const stamp = now.toISOString().slice(0, 16).replace("T", " ") + " UTC";
  out.push(`_Updated ${stamp} by ${tool} (${where})${br ? ` on branch \`${br}\`` : ""}. Rewritten after every turn; write your own notes above this section._`, "");
  if (s.goal) out.push(`**Goal:** ${s.goal}`, "");
  const list = (title, items, mark = "-") => {
    if (!items.length) return;
    out.push(`**${title}**`, ...items.map((i) => `${mark} ${i}`), "");
  };
  list("Latest requests", s.requests);
  list("Files changed", s.files.map((f) => `\`${f}\``));
  if (s.todos?.length) {
    out.push("**To-do**", ...s.todos.map((t) => `- [${t.status === "completed" ? "x" : " "}] ${clip(t.content ?? "", 30)}${t.status === "in_progress" ? " _(in progress)_" : ""}`), "");
  }
  if (s.reply) out.push(`**Last update from the agent:** ${s.reply}`, "");
  list("Recent errors", s.errors);
  out.push(END);
  return out.join("\n");
}

export const HEADER = `# Handoff

Shared notes so any AI agent (Claude Code, Codex, Gemini; cloud or local) can pick up this work.
Set up by AI Usage Controller. Agents: read this first, keep **Status** current, and commit this file with your work.

## Status

_Not written yet. Agents: say what's done, what's next, and any open problems, in a few lines._
`;

export function merge(existing, auto) {
  if (!existing) return `${HEADER}\n${auto}\n`;
  const a = existing.indexOf(START);
  const b = existing.indexOf(END);
  if (a >= 0 && b > a) return existing.slice(0, a) + auto + existing.slice(b + END.length);
  return `${existing.trimEnd()}\n\n${auto}\n`;
}

function main() {
  let input = {};
  try { input = JSON.parse(fs.readFileSync(0, "utf8") || "{}"); } catch {}
  const root = process.env.CLAUDE_PROJECT_DIR || input.cwd || process.cwd();
  const transcript = input.transcript_path;
  if (!transcript || !fs.existsSync(transcript)) return;
  const s = summarize(fs.readFileSync(transcript, "utf8").split("\n"), root);
  if (!s.goal && !s.files.length) return;
  const remote = !!process.env.CLAUDE_CODE_REMOTE;
  const file = path.join(root, ".aiuc", "handoff.md");
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const existing = fs.existsSync(file) ? fs.readFileSync(file, "utf8") : "";
  const next = merge(existing, render(s, { where: remote ? "cloud" : "local", br: branch(root) }));
  const tmp = `${file}.${process.pid}.tmp`;
  fs.writeFileSync(tmp, next);
  fs.renameSync(tmp, file);
  // In the cloud the container can disappear once usage runs out, so stage the
  // file to make sure it goes out with the agent's next commit.
  if (remote) {
    try { execFileSync("git", ["add", "--", file], { cwd: root, stdio: "ignore" }); } catch {}
  }
}

if (process.argv[1] && import.meta.url.endsWith(path.basename(process.argv[1]))) {
  try { main(); } catch {}
}
