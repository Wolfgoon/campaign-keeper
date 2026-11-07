<script lang="ts">
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import { call, toast, MODELS } from "../lib/api";
  import Sound from "./Sound.svelte";
  import Voices from "./Voices.svelte";

  // Someone at a friend's table has no game of their own to connect: `playerOnly` keeps it to what's theirs.
  let { cfg, save, close, playerOnly = false } = $props();
  let claude = $state<any>({});
  let models = $state<string[]>([]);
  let installing = $state(false);

  const PRESETS = [["OpenRouter", "https://openrouter.ai/api/v1"], ["Ollama", "http://localhost:11434/v1"], ["LM Studio", "http://localhost:1234/v1"]];

  let version = $state("");

  onMount(async () => {
    claude = await call("claude_status");
    version = await getVersion();
  });

  async function install() {
    installing = true;
    try {
      toast(await call("claude_install"));
      claude = await call("claude_status");
    } catch (e) {
      toast(e);
    }
    installing = false;
  }

  async function fetchModels() {
    await save();
    try {
      models = await call("list_models");
    } catch (e) {
      toast(e);
    }
  }
</script>

<div class="modal-back">
  <div class="card col scroll" style="margin:auto;width:min(620px,94%);max-height:92%">
    <div class="row"><h2 class="grow" style="margin:0">Settings</h2><span class="small muted">Version {version}</span><button onclick={() => { save(); close(); }}>Done</button></div>

    {#if playerOnly}
      <label class="field"><span>Your name</span><input bind:value={cfg.player_name} /></label>
      <h3>Music</h3>
      <Sound {cfg} />
      <h3>Read aloud</h3>
      <Voices {cfg} />
    {:else}
    <h3>Connect to Claude</h3>
    <p class="small muted">The Play and Scribe tabs use this. Everything else works without it.</p>
    <label class="check"><input type="radio" bind:group={cfg.engine} value="claude_code" style="width:auto" /> <span>Claude Code: your Claude account and plan, no API key</span></label>
    <label class="check"><input type="radio" bind:group={cfg.engine} value="api" style="width:auto" /> <span>Anthropic API key: pay per use</span></label>
    <label class="check"><input type="radio" bind:group={cfg.engine} value="other" style="width:auto" /> <span>Other models: OpenRouter, or your own with Ollama or LM Studio</span></label>

    {#if cfg.engine === "claude_code"}
      <p class="small">{claude.path ? `✔ Found ${claude.path}` : "✖ Claude Code wasn't found."}</p>
      <div class="row">
        {#if !claude.path}<button class="primary" disabled={installing} onclick={install}>{installing ? "Installing…" : "Set up Claude Code for me"}</button>{/if}
        <button onclick={() => call("claude_sign_in").catch(toast)}>Sign in to Claude Code…</button>
      </div>
      <label class="field"><span>Its location, if it wasn't found</span><input bind:value={cfg.claude_path} /></label>
    {:else if cfg.engine === "api"}
      <label class="field"><span>API key</span><input type="password" bind:value={cfg.api_key} placeholder="sk-ant-…" /></label>
      <label class="check small"><input type="checkbox" bind:checked={cfg.remember_key} /> <span>Remember the key on this computer</span></label>
      <label class="check small"><input type="checkbox" bind:checked={cfg.fallbacks} /> <span>If the model declines a request, retry it on a fallback model. Safety classifiers can occasionally flag ordinary fiction; this keeps the game going.</span></label>
    {:else}
      <div class="row">{#each PRESETS as [n, u]}<button onclick={() => (cfg.other_url = u)}>{n}</button>{/each}</div>
      <label class="field"><span>Address</span><input bind:value={cfg.other_url} /></label>
      <label class="field"><span>API key (not needed for Ollama or LM Studio)</span><input type="password" bind:value={cfg.other_key} /></label>
      <label class="field"><span>Model</span><input bind:value={cfg.other_model} list="model-list" /></label>
      <datalist id="model-list">{#each models as m}<option value={m}></option>{/each}</datalist>
      <button onclick={fetchModels}>List models</button>
      <p class="small muted">Models on your own computer need a large context window (32,000 tokens or more). Smaller ones may forget details or the app's tags.</p>
    {/if}

    {#if cfg.engine !== "other"}
      <label class="field"><span>Model</span>
        <select bind:value={cfg.model}>{#each MODELS as m}<option value={m.id}>{m.name}</option>{/each}</select>
      </label>
      <p class="small muted">{MODELS.find((m) => m.id === cfg.model)?.note}</p>
      <label class="field"><span>Thinking effort</span>
        <select bind:value={cfg.effort}>
          <option value="low">Low: quickest replies</option><option value="medium">Medium: good default</option><option value="high">High: more careful</option><option value="xhigh">Extra high</option><option value="max">Max: slowest, most thorough</option>
        </select>
      </label>
    {/if}
    <label class="field"><span>Max output tokens per reply. A ceiling, not a target; thinking counts toward it.</span><input type="number" min="1000" step="1000" bind:value={cfg.max_tokens} /></label>

    <h3>Memory</h3>
    <label class="check"><input type="checkbox" bind:checked={cfg.memory_enabled} /> <span>Long-term memory: recall, the Keeper and condensing</span></label>
    {#if cfg.memory_enabled}
      <label class="field"><span>Condense older turns past this context size: {Math.round(cfg.context_limit / 1000)}k tokens. Lower is cheaper per turn.</span><input type="range" min="20000" max="400000" step="10000" bind:value={cfg.context_limit} /></label>
      <label class="field"><span>Memories recalled per turn: {cfg.recall_k}</span><input type="range" min="0" max="15" bind:value={cfg.recall_k} /></label>
      <label class="field"><span>The Keeper runs every {cfg.keeper_every} replies</span><input type="range" min="1" max="15" bind:value={cfg.keeper_every} /></label>
      <label class="field"><span>Campaign files</span>
        <select bind:value={cfg.context_mode}><option value="smart">Smart: core files whole, the rest recalled when relevant</option><option value="full">Everything, every turn</option></select>
      </label>
      <label class="field"><span>Helper model for background jobs ("auto" uses the cheapest fit)</span><input bind:value={cfg.helper_model} /></label>
      <label class="check small"><input type="checkbox" bind:checked={cfg.continuity_check} /> <span>Check each reply against memory for contradictions (uses the helper once per turn)</span></label>
      <label class="check small"><input type="checkbox" bind:checked={cfg.playstyle_enabled} /> <span>Learn how I like to play, across campaigns</span></label>
    {/if}
    {/if}

    <h3>Appearance</h3>
    <label class="check"><input type="checkbox" bind:checked={cfg.dark_mode} onchange={save} /> <span>Dark mode</span></label>
    <label class="field"><span>Size: {Math.round(cfg.ui_scale * 100)}%</span><input type="range" min="0.8" max="1.5" step="0.05" bind:value={cfg.ui_scale} onchange={save} /></label>
    <label class="field"><span>Reading speed: {cfg.tts_speed.toFixed(2)}x</span><input type="range" min="0.6" max="1.6" step="0.05" bind:value={cfg.tts_speed} /></label>
  </div>
</div>
