# Adversarial Review Arena 🤖⚔⚡

> Tauri app that pits **Claude Code** against **Codex** in a head-to-head adversarial code review battle.

## What It Does

1. Pick a target source file.
2. Choose which agent goes first (Claude Code or Codex).
3. Set the number of review turns.
4. Hit **Start Battle** — the two agents take turns reviewing the file and then critiquing each other's reviews, getting progressively more adversarial each turn.

The UI shows a split view:

```
┌─────────────────────────────────────────────────┐
│  Config Bar: [File] [First Agent] [Turns] [▶]   │
├─────────────────────────────────────────────────┤
│  Status: ⚡ Turn 2/3 — battle in progress        │
├──────────────────────┬──────────────────────────┤
│  🤖 Claude Code      │  ⚡ Codex               │
│                      │                          │
│  Turn 1 review...    │  Turn 1 counter-review.. │
│  Turn 2 review...    │  Turn 2 counter-review.. │
│                      │                          │
├──────────────────────┴──────────────────────────┤
│  Battle Log                                      │
│  10:42 Turn 1 starting...                        │
│  10:43 claude completed review for turn 1        │
│  10:45 Turn 1 complete — both agents reviewed   │
└─────────────────────────────────────────────────┘
```

## Requirements

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) (stable toolchain)
- [Tauri CLI v2](https://tauri.app/) (`npm install` will pull it)
- [Claude Code CLI](https://claude.ai/code) installed and authenticated
- [Codex CLI](https://github.com/openai/codex) installed and authenticated

## Quick Start

```bash
cd adversarial-review-arena

# Install frontend deps
npm install

# Run in dev mode (launches both Vite + Tauri)
npm run tauri dev

# Or build a standalone app
npm run tauri build
```

## How It Works

### Review Flow

Each **turn** consists of two phases:

1. **Review phase**: The first agent reviews the target file (or counter-reviews the previous turn's last review).
2. **Counter-review phase**: The second agent reviews the same file AND critiques the first agent's review.

The next turn flips: the second agent reviews first, then the first agent counter-reviews.

Each turn gets progressively more adversarial — agents are prompted to find flaws the other missed, call out incorrect claims, and dig deeper.

### Prompts

The Rust backend (`src-tauri/src/lib.rs`) constructs adversarial review prompts that instruct each agent to:

- Find logic bugs, edge cases, error handling issues
- Identify security vulnerabilities
- Spot performance problems (O(n²) traps, leaks)
- Call out maintainability issues (naming, structure, DRY)
- Check for idiomatic/style violations
- Critique the opponent's review for accuracy and completeness

### Agent Invocation

Agents are invoked via their CLIs as shell commands through Tauri's `tauri-plugin-shell`:

- **Claude Code**: `claude --print --output-format text --input-format text "<prompt>"`
- **Codex**: `codex --approval-mode never --quiet "<prompt>"`

## Configuration

Custom system prompts can be added per-agent in the Rust `ReviewConfig` struct. The UI exposes:

| Setting       | Description                              |
|---------------|------------------------------------------|
| Target File   | File to review (file picker dialog)      |
| First Agent   | Claude Code or Codex goes first          |
| Max Turns     | Total review rounds (1-20)               |

## Architecture

```
adversarial-review-arena/
├── package.json              # Frontend deps
├── vite.config.ts            # Vite config
├── tsconfig.json
├── src/                      # Frontend (TypeScript + HTML + CSS)
│   ├── index.html
│   ├── main.ts              # UI logic, Tauri event handling
│   └── styles.css           # Dark theme split-pane layout
└── src-tauri/                # Backend (Rust)
    ├── Cargo.toml
    ├── build.rs
    ├── tauri.conf.json      # Tauri config, shell permissions
    ├── icons/
    └── src/
        ├── main.rs          # Entry point
        └── lib.rs           # Core: ReviewConfig, prompt builder, agent runner, orchestration loop
```

## Notes

- This is a prototype/sample app for the [toybox](https://github.com/nekowasabi/toybox) repo.
- Both CLIs must be installed and authenticated beforehand.
- Reviews can take a while (each agent call is a full LLM turn).
- The Cancel button stops after the current agent finishes its response.
- File content is read once at battle start; changes during the battle are NOT picked up.

## License

MIT — do whatever you want with this sample code.