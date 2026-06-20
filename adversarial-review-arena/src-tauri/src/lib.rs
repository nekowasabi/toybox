use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use tauri::{Emitter, State};
use tauri_plugin_shell::ShellExt;

// ─── Config Types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub command: String,
    pub args: Vec<String>,
    pub extra_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaProfile {
    /// "auto" = use each CLI's default language; "ja", "en", etc. = explicit override
    pub language: String,
    pub first_agent: String,
    pub max_turns: u32,
    pub claude: AgentConfig,
    pub codex: AgentConfig,
    /// Name of the prompt template file (e.g. "default", "freestyle")
    pub prompt_template: String,
}

impl Default for ArenaProfile {
    fn default() -> Self {
        Self {
            language: "auto".to_string(),
            first_agent: "claude".to_string(),
            max_turns: 3,
            claude: AgentConfig {
                command: "claude".to_string(),
                args: vec![
                    "--print".to_string(),
                    "--output-format".to_string(),
                    "text".to_string(),
                    "--input-format".to_string(),
                    "text".to_string(),
                ],
                extra_prompt: None,
            },
            codex: AgentConfig {
                command: "codex".to_string(),
                args: vec!["--approval-mode".to_string(), "never".to_string(), "--quiet".to_string()],
                extra_prompt: None,
            },
            prompt_template: "default".to_string(),
        }
    }
}

// ─── Runtime Types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewConfig {
    /// Path to the target file being reviewed
    pub target_file: String,
    /// Profile name (e.g. "default", "slack", "freestyle", "battle")
    pub profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewTurn {
    pub turn_number: u32,
    pub reviewer: String,
    pub prompt: String,
    pub review_text: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaState {
    pub config: ReviewConfig,
    pub profile: ArenaProfile,
    pub turns: Vec<ReviewTurn>,
    pub current_turn: u32,
    pub is_running: bool,
    pub is_complete: bool,
}

// ─── Config Loading ───────────────────────────────────────────────────────────

fn config_dir() -> PathBuf {
    // Walk up from the executable to find .config directory, fallback to CWD
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        while let Some(d) = dir {
            let candidate = d.join(".config");
            if candidate.is_dir() {
                return candidate;
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }
    // Fallback: current working directory
    PathBuf::from(".config")
}

fn load_profile(profile_name: &str) -> Result<ArenaProfile, String> {
    let cfg_dir = config_dir();
    let profile_dir = cfg_dir.join(profile_name);

    if !profile_dir.is_dir() {
        return Err(format!(
            "Profile '{}' not found. Expected at: {}",
            profile_name,
            profile_dir.display()
        ));
    }

    // Load arena.json
    let arena_json_path = profile_dir.join("arena.json");
    let arena_json = std::fs::read_to_string(&arena_json_path)
        .map_err(|e| format!("Failed to read {}: {e}", arena_json_path.display()))?;

    let mut profile: ArenaProfile = serde_json::from_str(&arena_json)
        .map_err(|e| format!("Failed to parse {}: {e}", arena_json_path.display()))?;

    // Load prompt template
    let prompt_path = profile_dir.join("prompt.txt");
    // We don't store the template in the profile struct; it's loaded at review time

    Ok(profile)
}

fn load_prompt_template(profile_name: &str) -> Result<String, String> {
    let cfg_dir = config_dir();
    let prompt_path = cfg_dir.join(profile_name).join("prompt.txt");
    std::fs::read_to_string(&prompt_path)
        .map_err(|e| format!("Failed to read prompt template {}: {e}", prompt_path.display()))
}

fn list_profiles() -> Vec<String> {
    let cfg_dir = config_dir();
    let mut profiles = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&cfg_dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                // Only include dirs that have arena.json
                if entry.path().join("arena.json").exists() {
                    profiles.push(name);
                }
            }
        }
    }
    profiles.sort();
    profiles
}

// ─── Prompt Builder ───────────────────────────────────────────────────────────

fn build_language_instruction(language: &str) -> String {
    match language {
        "auto" => String::new(), // Let each CLI use its default
        "ja" => "\n\n重要: 日本語で回答してください。".to_string(),
        "en" => "\n\nImportant: Respond in English.".to_string(),
        lang => format!("\n\nImportant: Respond in {lang}."),
    }
}

fn build_prompt(
    template: &str,
    target_file: &str,
    file_content: &str,
    reviewer_name: &str,
    reviewee_name: &str,
    turn: u32,
    max_turns: u32,
    language: &str,
    extra_prompt: Option<&str>,
) -> String {
    let language_instruction = build_language_instruction(language);
    let extra = if let Some(e) = extra_prompt {
        format!("\n{e}\n")
    } else {
        String::new()
    };

    template
        .replace("{reviewer_name}", reviewer_name)
        .replace("{reviewee_name}", reviewee_name)
        .replace("{target_file}", target_file)
        .replace("{file_content}", file_content)
        .replace("{turn}", &turn.to_string())
        .replace("{max_turns}", &max_turns.to_string())
        .replace("{language_instruction}", &language_instruction)
        .replace("{extra_prompt}", &extra)
}

// ─── Agent Runner ─────────────────────────────────────────────────────────────

async fn run_agent(
    agent_cfg: &AgentConfig,
    prompt: &str,
    app: &tauri::AppHandle,
) -> Result<String, String> {
    let mut cmd = app.shell().command(&agent_cfg.command);
    for arg in &agent_cfg.args {
        cmd = cmd.arg(arg);
    }
    cmd = cmd.arg(prompt);

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("Failed to run {}: {e}", agent_cfg.command))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(format!(
            "{} exited with {}: {}",
            agent_cfg.command,
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

// ─── File Reading ────────────────────────────────────────────────────────────

fn read_file_content(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("Failed to read {path}: {e}"))
}

// ─── State ───────────────────────────────────────────────────────────────────

struct AppState {
    config: Mutex<ReviewConfig>,
    profile: Mutex<ArenaProfile>,
    turns: Mutex<Vec<ReviewTurn>>,
    current_turn: Mutex<u32>,
    is_running: Mutex<bool>,
    is_complete: Mutex<bool>,
    cancel_flag: AtomicU32,
}

impl AppState {
    fn new() -> Self {
        Self {
            config: Mutex::new(ReviewConfig {
                target_file: String::new(),
                profile: "default".to_string(),
            }),
            profile: Mutex::new(ArenaProfile::default()),
            turns: Mutex::new(Vec::new()),
            current_turn: Mutex::new(0),
            is_running: Mutex::new(false),
            is_complete: Mutex::new(false),
            cancel_flag: AtomicU32::new(0),
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancel_flag.load(Ordering::SeqCst) != 0
    }

    fn set_cancel(&self, val: bool) {
        self.cancel_flag.store(if val { 1 } else { 0 }, Ordering::SeqCst);
    }

    fn snapshot(&self) -> ArenaState {
        ArenaState {
            config: self.config.lock().unwrap().clone(),
            profile: self.profile.lock().unwrap().clone(),
            turns: self.turns.lock().unwrap().clone(),
            current_turn: *self.current_turn.lock().unwrap(),
            is_running: *self.is_running.lock().unwrap(),
            is_complete: *self.is_complete.lock().unwrap(),
        }
    }
}

// ─── Tauri Commands ───────────────────────────────────────────────────────────

#[tauri::command]
fn get_state(state: State<'_, AppState>) -> ArenaState {
    state.snapshot()
}

#[tauri::command]
fn list_config_profiles() -> Vec<String> {
    list_profiles()
}

#[tauri::command]
fn load_profile_cmd(profile_name: String, state: State<'_, AppState>) -> Result<ArenaState, String> {
    let profile = load_profile(&profile_name)?;
    *state.profile.lock().unwrap() = profile;
    *state.config.lock().unwrap() = ReviewConfig {
        target_file: state.config.lock().unwrap().target_file.clone(),
        profile: profile_name,
    };
    Ok(state.snapshot())
}

#[tauri::command]
fn set_config(config: ReviewConfig, state: State<'_, AppState>) -> Result<ArenaState, String> {
    // Load the profile from .config
    let profile = load_profile(&config.profile)?;
    *state.profile.lock().unwrap() = profile;
    *state.config.lock().unwrap() = config;
    *state.turns.lock().unwrap() = Vec::new();
    *state.current_turn.lock().unwrap() = 0;
    *state.is_running.lock().unwrap() = false;
    *state.is_complete.lock().unwrap() = false;
    Ok(state.snapshot())
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

    let config = state.config.lock().unwrap().clone();
    let profile = state.profile.lock().unwrap().clone();

    if config.target_file.is_empty() {
        return Err("No target file selected".to_string());
    }

    // Reset state
    {
        *state.turns.lock().unwrap() = Vec::new();
        *state.current_turn.lock().unwrap() = 0;
        *state.is_running.lock().unwrap() = true;
        *state.is_complete.lock().unwrap() = false;
    }

    let file_content = read_file_content(&config.target_file)?;
    let prompt_template = load_prompt_template(&config.prompt_template)?;

    // Determine agent order
    let (first_name, first_cfg, second_name, second_cfg) = match profile.first_agent.as_str() {
        "claude" => ("claude", &profile.claude, "codex", &profile.codex),
        "codex" => ("codex", &profile.codex, "claude", &profile.claude),
        _ => return Err("first_agent must be 'claude' or 'codex'".to_string()),
    };

    for turn in 1..=profile.max_turns {
        if state.is_cancelled() {
            *state.is_running.lock().unwrap() = false;
            app.emit("review-cancelled", &state.snapshot()).ok();
            return Ok(state.snapshot());
        }

        *state.current_turn.lock().unwrap() = turn;
        app.emit("review-turn-start", &state.snapshot()).ok();

        // ── First agent reviews ──────────────────────────────────────────
        let prompt1 = build_prompt(
            &prompt_template,
            &config.target_file,
            &file_content,
            first_name,
            second_name,
            turn,
            profile.max_turns,
            &profile.language,
            first_cfg.extra_prompt.as_deref(),
        );

        app.emit("review-agent-start", serde_json::json!({
            "agent": first_name,
            "turn": turn,
            "phase": "review"
        })).ok();

        let review1 = run_agent(first_cfg, &prompt1, &app).await?;

        let turn1 = ReviewTurn {
            turn_number: turn,
            reviewer: first_name.to_string(),
            prompt: prompt1,
            review_text: review1,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        {
            state.turns.lock().unwrap().push(turn1);
            app.emit("review-turn-half", &state.snapshot()).ok();
        }

        if state.is_cancelled() {
            *state.is_running.lock().unwrap() = false;
            app.emit("review-cancelled", &state.snapshot()).ok();
            return Ok(state.snapshot());
        }

        // ── Second agent reviews ───────────────────────────────────────────
        let prompt2 = build_prompt(
            &prompt_template,
            &config.target_file,
            &file_content,
            second_name,
            first_name,
            turn,
            profile.max_turns,
            &profile.language,
            second_cfg.extra_prompt.as_deref(),
        );

        app.emit("review-agent-start", serde_json::json!({
            "agent": second_name,
            "turn": turn,
            "phase": "review"
        })).ok();

        let review2 = run_agent(second_cfg, &prompt2, &app).await?;

        let turn2 = ReviewTurn {
            turn_number: turn,
            reviewer: second_name.to_string(),
            prompt: prompt2,
            review_text: review2,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        {
            state.turns.lock().unwrap().push(turn2);
            app.emit("review-turn-complete", &state.snapshot()).ok();
        }
    }

    // Done
    *state.is_running.lock().unwrap() = false;
    *state.is_complete.lock().unwrap() = true;
    let final_state = state.snapshot();
    app.emit("review-complete", &final_state).ok();

    Ok(final_state)
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            get_state,
            set_config,
            start_review,
            cancel_review,
            list_config_profiles,
            load_profile_cmd,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}