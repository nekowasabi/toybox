# Adversarial Review Arena 🎤⚡🤖⚔

> Tauri app that pits **Claude Code** against **Codex** in a freestyle rap battle of adversarial code reviews.

## What It Does

1. Pick a **profile** (config preset) and a target source file.
2. Hit **Start Battle** — the two agents take turns reviewing the file, getting progressively more adversarial each turn.
3. Each review is delivered as a **rap verse** with real rhymes and technical content.

## Configuration — `.config/` directory

Profiles are stored as directories under `.config/`. Each profile has:

```
.config/
├── default/
│   ├── arena.json       # Settings: language, first_agent, max_turns, agent commands
│   └── prompt.txt       # Prompt template with {placeholders}
├── slack/
│   ├── arena.json       # Japanese language, freestyle template
│   └── prompt.txt       # Japanese prompt template
├── freestyle/
│   ├── arena.json       # English freestyle rap, 5 turns
│   └── prompt.txt       # English rap battle prompt
└── battle/
    ├── arena.json       # Intense battle, 10 turns, no rhyming required
    └── prompt.txt       # Standard adversarial review prompt
```

### arena.json

```json
{
  "language": "auto",       // "auto" = use each CLI's default language; "ja", "en", etc. = override
  "first_agent": "claude",  // who goes first: "claude" or "codex"
  "max_turns": 3,           // total review rounds
  "claude": {
    "command": "claude",
    "args": ["--print", "--output-format", "text", "--input-format", "text"],
    "extra_prompt": null    // extra instructions appended to prompt (or null)
  },
  "codex": {
    "command": "codex",
    "args": ["--approval-mode", "never", "--quiet"],
    "extra_prompt": null
  },
  "prompt_template": "default"  // references prompt.txt in the same directory
}
```

### prompt.txt placeholders

| Placeholder | Replaced with |
|---|---|
| `{reviewer_name}` | "claude" or "codex" |
| `{reviewee_name}` | The opponent's name |
| `{target_file}` | Path to the target file |
| `{file_content}` | Contents of the target file |
| `{turn}` | Current turn number (1-based) |
| `{max_turns}` | Total turns from config |
| `{language_instruction}` | Auto-generated language directive (or empty for "auto") |
| `{extra_prompt}` | Contents of `extra_prompt` from arena.json (or empty) |

### Language behavior

- `"auto"` — No language directive is added. Each CLI outputs in whatever language it's configured to use.
- `"ja"` — Adds `重要: 日本語で回答してください。` to the prompt.
- `"en"` — Adds `Important: Respond in English.` to the prompt.
- Any other value — Adds `Important: Respond in {language}.` to the prompt.

### Adding a custom profile

1. Create `.config/my-profile/` directory
2. Add `arena.json` with your settings
3. Add `prompt.txt` with your prompt template
4. The profile appears automatically in the UI dropdown

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

## Architecture

```
adversarial-review-arena/
├── .config/                        # Profile-based configuration
│   ├── default/                    # Default profile
│   │   ├── arena.json              # Settings
│   │   └── prompt.txt              # Prompt template
│   ├── slack/                      # Japanese language profile
│   ├── freestyle/                  # English rap battle profile
│   └── battle/                     # Intense 10-turn battle
├── package.json
├── vite.config.ts
├── tsconfig.json
├── src/                            # Frontend (TypeScript + HTML + CSS)
│   ├── index.html                  # Split-pane UI with profile selector
│   ├── main.ts                     # UI logic, Tauri event handling
│   └── styles.css                   # Dark theme
└── src-tauri/                      # Backend (Rust)
    ├── Cargo.toml
    ├── build.rs
    ├── tauri.conf.json             # Tauri config, shell permissions
    └── src/
        ├── main.rs                  # Entry point
        └── lib.rs                   # Config loading, prompt builder, agent runner, orchestration
```

## Review Flow

Each **turn** consists of two phases (no counter-verse for now — just alternating reviews):

1. First agent reviews the target file
2. Second agent reviews the same file

The next turn repeats with the same order. Each turn uses the same prompt template, so the adversarial nature comes from the prompt instructions themselves.

## Notes

- This is a prototype/sample app for the [toybox](https://github.com/nekowasabi/toybox) repo.
- Both CLIs must be installed and authenticated beforehand.
- Reviews can take a while (each agent call is a full LLM turn).
- The Cancel button stops after the current agent finishes its response.
- File content is read once at battle start; changes during the battle are NOT picked up.

## License

MIT — do whatever you want with this sample code.