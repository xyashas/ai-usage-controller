# Privacy, in plain language

**What the app reads.** The usage logs that Claude Code, Codex and Gemini CLI already write on your computer. From each line it keeps the time, the tool, the model, token counts and a session id.

**What it never keeps.** Your prompts, the AI's replies, your code and file contents. For loop detection it keeps a short label such as `Edit:src/app.ts`. That label stays on your computer.

**Where it's stored.** A local database in your app-data folder. You can delete it any time.

**What leaves your computer.**

- *Nothing*, unless you turn on sync or join a team.
- With sync on: the time, tool, model, token counts, estimated cost and session id of each AI request. Never prompts, code, file names, folder names or project names.
- If you use Claude's live gauge, the app asks Anthropic for your own usage percentage, using the Claude Code login already on your computer. That request goes only to Anthropic.
- If you connect your Claude account, you sign in to claude.ai in a window inside the app. That window keeps its own claude.ai login (like a browser would) and once a minute asks claude.ai for your plan usage. The app never copies the login out of that window, and those requests go only to claude.ai. Disconnecting in Settings signs that window out and clears it.
- Licence checks send your licence key and device name to Lemon Squeezy.

**Team admins see** each member's request counts, estimated cost, plans, last active date and plan recommendations. They don't see prompts, code or projects.

**Anthropic Admin keys** that a team pastes into the dashboard are encrypted at rest and only sent to Anthropic.
