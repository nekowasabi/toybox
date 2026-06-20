import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { readFile } from "@tauri-apps/plugin-fs";

// ─── DOM Elements ─────────────────────────────────────────────────────────────
const targetFileInput = document.getElementById("target-file") as HTMLInputElement;
const browseBtn = document.getElementById("browse-btn") as HTMLButtonElement;
const firstAgentSelect = document.getElementById("first-agent") as HTMLSelectElement;
const maxTurnsInput = document.getElementById("max-turns") as HTMLInputElement;
const startBtn = document.getElementById("start-btn") as HTMLButtonElement;
const cancelBtn = document.getElementById("cancel-btn") as HTMLButtonElement;
const statusBar = document.getElementById("status-bar") as HTMLDivElement;
const claudeOutput = document.getElementById("claude-output") as HTMLDivElement;
const codexOutput = document.getElementById("codex-output") as HTMLDivElement;
const claudeTurnCount = document.getElementById("claude-turn-count") as HTMLSpanElement;
const codexTurnCount = document.getElementById("codex-turn-count") as HTMLSpanElement;
const logContent = document.getElementById("log-content") as HTMLDivElement;

// ─── Helpers ──────────────────────────────────────────────────────────────────
function log(msg: string, level: string = "info") {
  const time = new Date().toLocaleTimeString();
  const entry = document.createElement("div");
  entry.className = "log-entry";
  entry.innerHTML = `<span class="log-time">${time}</span><span class="log-${level}">${msg}</span>`;
  logContent.appendChild(entry);
  logContent.scrollTop = logContent.scrollHeight;
}

function setStatus(text: string, cls: string) {
  statusBar.innerHTML = `<span class="${cls}">${text}</span>`;
}

function renderReview(agent: string, turn: number, reviewText: string, timestamp: string) {
  const container = agent === "claude" ? claudeOutput : codexOutput;
  const countEl = agent === "claude" ? claudeTurnCount : codexTurnCount;

  // Remove placeholder
  const placeholder = container.querySelector(".placeholder");
  if (placeholder) placeholder.remove();

  const card = document.createElement("div");
  card.className = "review-card";
  card.innerHTML = `
    <div class="review-card-header">
      <span class="review-card-title">Turn ${turn} — ${agent === "claude" ? "🤖 Claude Code" : "⚡ Codex"}</span>
      <span class="review-card-meta">${new Date(timestamp).toLocaleString()}</span>
    </div>
    <div class="review-text"></div>
  `;
  card.querySelector(".review-text")!.textContent = reviewText;
  container.appendChild(card);
  container.scrollTop = container.scrollHeight;

  // Update count
  const count = container.querySelectorAll(".review-card").length;
  countEl.textContent = `${count} review${count !== 1 ? "s" : ""}`;
}

function clearPanes() {
  claudeOutput.innerHTML = '<div class="placeholder">Claude Code reviews will appear here</div>';
  codexOutput.innerHTML = '<div class="placeholder">Codex reviews will appear here</div>';
  claudeTurnCount.textContent = "0 reviews";
  codexTurnCount.textContent = "0 reviews";
}

function setRunning(running: boolean) {
  startBtn.disabled = running;
  cancelBtn.disabled = !running;
  targetFileInput.disabled = running;
  firstAgentSelect.disabled = running;
  maxTurnsInput.disabled = running;
  browseBtn.disabled = running;
}

// ─── Event listeners ──────────────────────────────────────────────────────────
let unlisteners: UnlistenFn[] = [];

async function setupListeners() {
  unlisteners.push(
    await listen("review-turn-start", (event) => {
      const state = event.payload as any;
      log(`Turn ${state.current_turn}/${state.config.max_turns} starting...`);
      setStatus(`⚡ Turn ${state.current_turn}/${state.config.max_turns} — battle in progress`, "status-running");
    })
  );

  unlisteners.push(
    await listen("review-agent-start", (event) => {
      const d = event.payload as any;
      log(`${d.agent} is ${d.phase === "review" ? "reviewing" : "counter-reviewing"} (turn ${d.turn})...`);
    })
  );

  unlisteners.push(
    await listen("review-turn-half", (event) => {
      const state = event.payload as any;
      const lastTurn = state.turns[state.turns.length - 1];
      if (lastTurn) {
        renderReview(lastTurn.reviewer, lastTurn.turn_number, lastTurn.review_text, lastTurn.timestamp);
        log(`${lastTurn.reviewer} completed review for turn ${lastTurn.turn_number}`, "success");
      }
    })
  );

  unlisteners.push(
    await listen("review-turn-complete", (event) => {
      const state = event.payload as any;
      const lastTurn = state.turns[state.turns.length - 1];
      if (lastTurn) {
        renderReview(lastTurn.reviewer, lastTurn.turn_number, lastTurn.review_text, lastTurn.timestamp);
        log(`Turn ${lastTurn.turn_number} complete — both agents have reviewed`, "success");
      }
    })
  );

  unlisteners.push(
    await listen("review-complete", (event) => {
      const state = event.payload as any;
      log(`Battle complete! ${state.turns.length} reviews generated.`, "success");
      setStatus(`✅ Battle complete — ${state.turns.length} reviews across ${state.config.max_turns} turns`, "status-complete");
      setRunning(false);
    })
  );

  unlisteners.push(
    await listen("review-cancelled", (event) => {
      log("Battle cancelled by user.", "warn");
      setStatus("■ Cancelled", "status-error");
      setRunning(false);
    })
  );
}

// ─── Button handlers ─────────────────────────────────────────────────────────
browseBtn.addEventListener("click", async () => {
  const selected = await open({
    multiple: false,
    filters: [{ name: "All Files", extensions: ["*"] }],
  });
  if (selected) {
    targetFileInput.value = selected as string;
    log(`Selected file: ${selected}`);
  }
});

startBtn.addEventListener("click", async () => {
  const targetFile = targetFileInput.value.trim();
  if (!targetFile) {
    log("Please select a target file first.", "error");
    return;
  }

  const config = {
    target_file: targetFile,
    first_agent: firstAgentSelect.value,
    max_turns: parseInt(maxTurnsInput.value) || 3,
    claude_system_prompt: null as string | null,
    codex_system_prompt: null as string | null,
  };

  clearPanes();
  logContent.innerHTML = "";
  log(`Starting battle: ${config.first_agent} goes first, ${config.max_turns} turns, file: ${config.target_file}`);
  setStatus("⚡ Battle starting...", "status-running");
  setRunning(true);

  try {
    await invoke("set_config", { config });
    await invoke("start_review");
  } catch (e: any) {
    log(`Error: ${e}`, "error");
    setStatus(`❌ Error: ${e}`, "status-error");
    setRunning(false);
  }
});

cancelBtn.addEventListener("click", async () => {
  log("Cancelling...");
  await invoke("cancel_review");
});

// ─── Init ─────────────────────────────────────────────────────────────────────
setupListeners().then(() => {
  log("Adversarial Review Arena ready.");
  log("Select a file, choose first agent and turns, then click Start Battle.");
});