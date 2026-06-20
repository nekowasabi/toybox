use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use tauri::{Emitter, State};
use tauri_plugin_shell::ShellExt;

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewConfig {
    /// Path to the target file being reviewed
    pub target_file: String,
    /// Which agent goes first: "claude" or "codex"
    pub first_agent: String,
    /// Total number of review rounds (each round = both agents review once)
    pub max_turns: u32,
    /// Extra prompt instructions for Claude
    pub claude_system_prompt: Option<String>,
    /// Extra prompt instructions for Codex
    pub codex_system_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewTurn {
    pub turn_number: u32,
    pub reviewer: String,
    pub reviewee: String,
    pub prompt: String,
    pub review_text: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaState {
    pub config: ReviewConfig,
    pub turns: Vec<ReviewTurn>,
    pub current_turn: u32,
    pub is_running: bool,
    pub is_complete: bool,
}

impl Default for ArenaState {
    fn default() -> Self {
        Self {
            config: ReviewConfig {
                target_file: String::new(),
                first_agent: "claude".to_string(),
                max_turns: 3,
                claude_system_prompt: None,
                codex_system_prompt: None,
            },
            turns: Vec::new(),
            current_turn: 0,
            is_running: false,
            is_complete: false,
        }
    }
}

// ─── State management ─────────────────────────────────────────────────────────

struct AppState {
    arena: Mutex<ArenaState>,
    cancel_flag: AtomicU32,
}

impl AppState {
    fn new() -> Self {
        Self {
            arena: Mutex::new(ArenaState::default()),
            cancel_flag: AtomicU32::new(0),
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancel_flag.load(Ordering::SeqCst) != 0
    }

    fn set_cancel(&self, val: bool) {
        self.cancel_flag.store(if val { 1 } else { 0 }, Ordering::SeqCst);
    }
}

// ─── Prompt builders ─────────────────────────────────────────────────────────

fn build_review_prompt(
    target_file: &str,
    file_content: &str,
    previous_review: Option<&str>,
    reviewer_name: &str,
    reviewee_name: &str,
    turn: u32,
    max_turns: u32,
    extra_prompt: Option<&str>,
) -> String {
    let base = format!(
        r#"You are {reviewer_name}, an extremely rigorous and adversarial code reviewer.
Your opponent is {reviewee_name}. You must find every flaw, vulnerability, bad practice,
and design issue in the code below. Be harsh but constructive. Do not hold back.

## Target File: {target_file}

```{file_content}
```

## Your Task
Perform a thorough adversarial code review. Cover:
1. Correctness — logic bugs, edge cases, error handling
2. Security — vulnerabilities, injection, unsafe patterns
3. Performance — unnecessary allocations, O(n²) traps, leaks
4. Maintainability — naming, structure, complexity, DRY violations
5. Style — idiomatic violations, consistency

Be specific. Reference line numbers. Suggest concrete fixes.
"#
    );

    let context = if let Some(prev) = previous_review {
        format!(
            r#"
## Previous Review by {reviewee_name} (Turn {}/{max_turns})

{prev}

## Your Counter-Review
{reviewee_name} produced the review above. Now it's YOUR turn (Turn {}/{max_turns}).
Critique their review AND the original code. Find things they missed, things they got
wrong, and new issues they didn't mention. Be even more thorough than they were.
"#,
            turn.saturating_sub(1),
            turn,
        )
    } else {
        format!(
            "\n## Your Review (Turn {}/{max_turns})\nProvide your initial review.\n",
            turn, max_turns
        )
    };

    let extra = if let Some(e) = extra_prompt {
        format!("\n## Additional Instructions\n{e}\n")
    } else {
        String::new()
    };

    format!("{base}{context}{extra}\nOutput only your review in Markdown.")
}

// ─── Agent runners ────────────────────────────────────────────────────────────

async fn run_claude(prompt: &str, app: &tauri::AppHandle) -> Result<String, String> {
    let output = app
        .shell()
        .command("claude")
        .args([
            "--print",
            "--output-format", "text",
            "--input-format", "text",
            prompt,
        ])
        .output()
        .await
        .map_err(|e| format!("Failed to run claude: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(format!(
            "claude exited with {}: {}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

async fn run_codex(prompt: &str, app: &tauri::AppHandle) -> Result<String, String> {
    let output = app
        .shell()
        .command("codex")
        .args([
            "--approval-mode", "never",
            "--quiet",
            prompt,
        ])
        .output()
        .await
        .map_err(|e| format!("Failed to run codex: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(format!(
            "codex exited with {}: {}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

async fn run_agent(
    agent: &str,
    prompt: &str,
    app: &tauri::AppHandle,
) -> Result<String, String> {
    match agent {
        "claude" => run_claude(prompt, app).await,
        "codex" => run_codex(prompt, app).await,
        _ => Err(format!("Unknown agent: {agent}")),
    }
}

// ─── File reading ────────────────────────────────────────────────────────────

fn read_file_content(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("Failed to read {path}: {e}"))
}

// ─── Tauri commands ──────────────────────────────────────────────────────────

#[tauri::command]
fn get_state(state: State<'_, AppState>) -> ArenaState {
    state.arena.lock().unwrap().clone()
}

#[tauri::command]
fn set_config(config: ReviewConfig, state: State<'_, AppState>) -> ArenaState {
    let mut arena = state.arena.lock().unwrap();
    arena.config = config;
    arena.turns.clear();
    arena.current_turn = 0;
    arena.is_running = false;
    arena.is_complete = false;
    arena.clone()
}

#[tauri::command]
fn cancel_review(state: State<'_, AppState>) {
    state.set_cancel(true);
}

#[tauri::command]
async fn start_review(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<ArenaState, String> {
    state.set_cancel(false);

    let config = {
        let arena = state.arena.lock().unwrap();
        if arena.config.target_file.is_empty() {
            return Err("No target file selected".to_string());
        }
        arena.config.clone()
    };

    // Mark as running
    {
        let mut arena = state.arena.lock().unwrap();
        arena.is_running = true;
        arena.is_complete = false;
        arena.turns.clear();
        arena.current_turn = 0;
    }

    let file_content = read_file_content(&config.target_file)?;

    let agents = match config.first_agent.as_str() {
        "claude" => ("claude", "codex"),
        "codex" => ("codex", "claude"),
        _ => return Err("first_agent must be 'claude' or 'codex'".to_string()),
    };

    let (first, second) = agents;

    for turn in 1..=config.max_turns {
        // Check cancellation
        if state.is_cancelled() {
            let mut arena = state.arena.lock().unwrap();
            arena.is_running = false;
            app.emit("review-cancelled", &*arena).ok();
            return Ok(arena.clone());
        }

        // ── First agent reviews ────────────────────────────────────────────
        let prev_review = state
            .arena
            .lock()
            .unwrap()
            .turns
            .last()
            .map(|t| t.review_text.clone());

        {
            let mut arena = state.arena.lock().unwrap();
            arena.current_turn = turn;
            app.emit("review-turn-start", &*arena).ok();
        }

        let prompt1 = build_review_prompt(
            &config.target_file,
            &file_content,
            prev_review.as_deref(),
            first,
            second,
            turn,
            config.max_turns,
            if first == "claude" {
                config.claude_system_prompt.as_deref()
            } else {
                config.codex_system_prompt.as_deref()
            },
        );

        app.emit("review-agent-start", serde_json::json!({
            "agent": first,
            "turn": turn,
            "phase": "review"
        })).ok();

        let review1 = run_agent(first, &prompt1, &app).await?;

        let turn1 = ReviewTurn {
            turn_number: turn,
            reviewer: first.to_string(),
            reviewee: second.to_string(),
            prompt: prompt1,
            review_text: review1,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        {
            let mut arena = state.arena.lock().unwrap();
            arena.turns.push(turn1.clone());
            app.emit("review-turn-half", &*arena).ok();
        }

        if state.is_cancelled() {
            let mut arena = state.arena.lock().unwrap();
            arena.is_running = false;
            app.emit("review-cancelled", &*arena).ok();
            return Ok(arena.clone());
        }

        // ── Second agent counter-reviews ─────────────────────────────────
        let prompt2 = build_review_prompt(
            &config.target_file,
            &file_content,
            Some(&turn1.review_text),
            second,
            first,
            turn,
            config.max_turns,
            if second == "claude" {
                config.claude_system_prompt.as_deref()
            } else {
                config.codex_system_prompt.as_deref()
            },
        );

        app.emit("review-agent-start", serde_json::json!({
            "agent": second,
            "turn": turn,
            "phase": "counter-review"
        })).ok();

        let review2 = run_agent(second, &prompt2, &app).await?;

        let turn2 = ReviewTurn {
            turn_number: turn,
            reviewer: second.to_string(),
            reviewee: first.to_string(),
            prompt: prompt2,
            review_text: review2,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        {
            let mut arena = state.arena.lock().unwrap();
            arena.turns.push(turn2.clone());
            app.emit("review-turn-complete", &*arena).ok();
        }
    }

    // Done
    let final_state = {
        let mut arena = state.arena.lock().unwrap();
        arena.is_running = false;
        arena.is_complete = true;
        arena.clone()
    };

    app.emit("review-complete", &final_state).ok();

    Ok(final_state)
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            get_state,
            set_config,
            start_review,
            cancel_review,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}