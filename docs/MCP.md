# Remote control for AI agents (MCP)

With remote control, an AI agent – Claude, Cursor, Codex, Gemini and others – can **look at and use myCommander**
for you: "Show me what is in my Downloads folder", "Mark all PDFs older than a year and move them to the archive",
"Find the file that mentions invoice 4711". The agent works in the **same two panels you see**, so you can
watch every step. It is **switched off until you turn it on**, and you decide how much it may do.

You need three steps: **1. turn it on · 2. tell your agent about myCommander · 3. try it.**

## 1. Turn it on in myCommander

Open **Settings (Cmd-,) ▸ General ▸ Remote control** and choose a level:

- **Off** (default) – do nothing – the door is closed.
- **Read only** – look: list folders, read text files, search, see both panels, take a screenshot.
- **Control** – additionally *navigate*: change folders, move the cursor, mark files, press keys, open menu items. It still **cannot change or run anything**.
- **Full** – additionally copy, move, rename, delete, create folders and run shell commands.

With **Full** there is a second switch, **"Ask me before an agent changes files"** – on by default.
Then every change shows a window that lists *exactly* what will happen, with **Allow** (Return) and **Deny** (Escape).
Nothing happens until you decide. Only you can press the button – an agent cannot.

> **Tip:** start with *Read only* or *Control*. Raise the level only for a job that needs it.

## 2. Tell your agent about myCommander

Every agent needs the same two facts: **command** = the path of myCommander, **argument** = `--mcp`.
The snippets below already contain the path of *this* copy of myCommander, so you can copy them as they are.
(Buttons at the top of this page copy the Claude Code command and the JSON for you.)

The path is: `{{BIN}}`

### Claude Code (terminal)

Run once in a terminal:

```
claude mcp add mycommander -- \
  "{{BIN}}" --mcp
```

Add `--scope user` after `mycommander` to have it in every project. Check with `claude mcp list`, or type `/mcp` inside Claude Code.

### Claude Desktop (the Mac app)

Open the file `~/Library/Application Support/Claude/claude_desktop_config.json` (Claude ▸ Settings ▸ Developer ▸ *Edit Config*),
add this (merge it into an existing `mcpServers` block), save, and **quit and reopen Claude Desktop**:

```json
{
  "mcpServers": {
    "mycommander": {
      "command": "{{BIN}}",
      "args": ["--mcp"]
    }
  }
}
```

### Cursor

Create or edit `~/.cursor/mcp.json` (for all projects) or `.cursor/mcp.json` (one project) with the same JSON block as above.
Then open *Cursor Settings ▸ MCP* and switch **mycommander** on.

### VS Code (GitHub Copilot, agent mode)

Create `.vscode/mcp.json` in your project (or use the command *MCP: Add Server…*). VS Code calls the block `servers`:

```json
{
  "servers": {
    "mycommander": {
      "type": "stdio",
      "command": "{{BIN}}",
      "args": ["--mcp"]
    }
  }
}
```

### Codex CLI

```
codex mcp add mycommander -- \
  "{{BIN}}" --mcp
```

or in `~/.codex/config.toml`:

```toml
[mcp_servers.mycommander]
command = "{{BIN}}"
args = ["--mcp"]
```

### Gemini CLI

Edit `~/.gemini/settings.json` and add the same `mcpServers` JSON block as for Claude Desktop. Check with `/mcp` inside Gemini CLI.

### Windsurf and other agents

Windsurf: `~/.codeium/windsurf/mcp_config.json`, same `mcpServers` block. Any other agent that supports MCP servers
("stdio" type) wants the same two things: **command** `{{BIN}}` and **arguments** `--mcp`. Look for "MCP", "tools" or "servers"
in its settings. If a path with spaces causes trouble, put the command in quotes or use the JSON form.

## 3. Try it

1. Keep myCommander **open and visible** (not minimised) with remote control on.
2. In your agent, say: **"Use myCommander to show me what is in the left and the right panel."**
3. The agent calls `get_state` and describes both panels. Then try: *"Go to my Downloads folder and list the biggest files"*
   or *"Take a screenshot of myCommander."*

If the agent answers "myCommander is not running, or remote control is switched off", see **Troubleshooting** below.

## What the agent can do (the tools)

- `get_state` (Read) – Both panels (folder, cursor, marked files, sort), open dialog, running job, status line.
- `list_entries` (Read) – Entries of a panel – or of any folder (`path`) without changing the panels.
- `read_text_file` (Read) – The beginning of a text file.
- `find_files` (Read) – Search by name pattern and/or text inside files.
- `places` (Read) – Mounted volumes (with free space), SSH servers, favorites.
- `screenshot` (Read) – A picture of the window.
- `get_help`, `list_menu_items` (Read) – The help texts; all menu items.
- `change_dir` (Control) – Show another folder (also `sftp://host/path`).
- `set_active_panel`, `set_cursor` (Control) – Choose a panel; put the cursor on a file.
- `select` (Control) – Mark files by name or wildcard (`*.jpg`), invert, clear.
- `configure_panel` (Control) – Sort order, listing format, hidden files, name filter.
- `press_keys`, `type_text` (Control) – Press keys like `Down Down Enter`, `Cmd+D`; type into a field.
- `run_menu_item` (Control) – Run a menu item such as *Command ▸ Swap panels*.
- `copy_or_move`, `delete`, `rename`, `make_directory` (Full) – Change files (with your approval, if switched on).
- `run_shell_command` (Full) – Run a command in a panel's folder; returns output and exit code.

Panels are called `active` (the one with the cursor), `other`, `left` or `right`.
Files are given by plain **name** inside the panel's folder. Deleted files are gone for good – myCommander has no trash.

## Safety and privacy – please read

- **Off by default**, and you can switch it off at any moment; the connection closes at once.
- **Local only.** The agent talks to myCommander through a private file (`~/Library/Application Support/myCommander/control.sock`)
  that only your Mac user can open. There is no network port and nothing is sent anywhere by myCommander.
- **But your agent's provider sees what the agent reads.** Even *Read only* lets an agent read any text file you can read
  and list any folder. What it reads becomes part of the conversation with the AI service you use. Don't enable remote
  control while sensitive folders are in play, or point the agent at harmless folders only.
- **Guard rails.** Below *Full*, an agent cannot press keys that change files or run programs (F4–F8, Ctrl-X, …),
  cannot confirm dialogs, cannot type into the editor, and cannot run a command line. At **every** level an agent can neither
  open **Settings** (it could raise its own level) nor answer your permission questions.
  These are protective rails, **not a sandbox**: at *Full* an agent can do what you can do – use *"Ask me before…"*.
- Your agent program may ask its *own* permission questions too (for example "Allow tool mycommander › delete?").
  myCommander marks every tool as read-only or destructive so the agent can do that properly.
- macOS may ask whether myCommander may access folders such as Documents or a network volume. That is the normal macOS privacy
  question; the agent can only reach what myCommander itself is allowed to reach.

## Troubleshooting

- *"myCommander is not running, or remote control is switched off"* – Start myCommander, open Settings ▸ General ▸ Remote control and choose a level other than *Off*.
- *"The tool … needs the remote-control level …"* – Raise the level in the same place – if you want the agent to be allowed to do that.
- *"myCommander did not answer within 50 s"* – Make the window visible (not minimised, not on a hidden desktop). Close any dialog that waits for you.
- The agent says it is *waiting for user approval* – Look at the myCommander window and press **Allow** or **Deny**.
- *"A permission question is open"* – That question is for you – answer it in the app.
- The server does not show up in the agent – Check the path (`{{BIN}}`) exists; restart the agent after editing its configuration; run `"{{BIN}}" --version` in a terminal.
- After updating myCommander the agent fails – The path stays the same in `/Applications`. If you moved the app, run the setup command again.
- Homebrew install – The app lives in `/Applications/myCommander.app`; the path above is correct.

## For developers

- Transport to the app: Unix socket `~/Library/Application Support/myCommander/control.sock`, one JSON object per line:
  request `{"id":1,"tool":"get_state","args":{}}`, answer `{"id":1,"ok":true,"result":{…}}` (screenshots add `png_base64`).
- `myCommander --mcp` speaks MCP (JSON-RPC 2.0 over stdio, protocol 2025-06-18; 2025-03-26 and 2024-11-05 accepted)
  and forwards `tools/call` to that socket. `tools/list` works even when the app is not running.
- Quick test without an agent:

  ```
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' \
    | "{{BIN}}" --mcp
  ```

- Set `MYCMD_CONFIG_DIR=/some/folder` to run an isolated instance (own settings and socket) for tests.
- The specification is in `docs/SPEC.md`, section 14 (REM).
