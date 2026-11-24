<script lang="ts">
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { call, toast, MODELS, AGY_MODELS } from "../lib/api";
  import { platform } from "../lib/platform.svelte";
  import Sound from "./Sound.svelte";
  import Voices from "./Voices.svelte";
  import Account from "./Account.svelte";

  // Someone at a friend's table has no game of their own to connect: `playerOnly` keeps it to what's theirs.
  let { cfg, save, close, playerOnly = false } = $props();
  let claude = $state<any>({});
  let agy = $state<any>({});
  let agyModels = $state<{ id: string; name: string; note?: string }[]>([...AGY_MODELS]);
  let models = $state<string[]>([]);
  let installing = $state(false);

  // Services that speak the OpenAI API and can carry a whole campaign: a large context window
  // (32k+ tokens) and enough tokens per minute for one turn. Picking one fills in the address and
  // a suggested model; "List models" shows what the service has today. Model names drift, so the
  // suggestion is only a starting point.
  type Preset = { name: string; url: string; model: string; note: string };
  const PRESET_GROUPS: [string, Preset[]][] = [
    ["Free online (sign up for a free key)", [
      { name: "Google Gemini", url: "https://generativelanguage.googleapis.com/v1beta/openai", model: "gemini-flash-latest", note: "Free key at aistudio.google.com. Huge context window and a generous free tier: the best free choice for long campaigns. Free-tier prompts may be used to improve Google's models." },
      { name: "OpenRouter (free models)", url: "https://openrouter.ai/api/v1", model: "", note: "Free key at openrouter.ai. Press List models and pick one ending in \":free\". About 50 free requests a day, 1,000 once you've ever added $10 of credit. Also has every paid model." },
      { name: "Mistral", url: "https://api.mistral.ai/v1", model: "mistral-large-latest", note: "Free \"Experiment\" plan at console.mistral.ai, no card needed. 128k context; rate limits are low but enough for play." },
      { name: "Z.ai GLM", url: "https://api.z.ai/api/paas/v4", model: "glm-4.5-flash", note: "Free key at z.ai. The Flash models are free, with a 128k context window." },
      { name: "Zhipu GLM (China)", url: "https://open.bigmodel.cn/api/paas/v4", model: "glm-4.5-flash", note: "Mainland China version of Z.ai, at bigmodel.cn. The Flash models are free." },
      { name: "Alibaba Qwen", url: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1", model: "qwen-plus", note: "Free starting quota for new accounts at Alibaba Cloud Model Studio. Long context." },
      { name: "Cohere", url: "https://api.cohere.ai/compatibility/v1", model: "command-a-03-2025", note: "Free trial key at dashboard.cohere.com: about 1,000 calls a month. 256k context." },
      { name: "NVIDIA NIM", url: "https://integrate.api.nvidia.com/v1", model: "", note: "Free developer access at build.nvidia.com, many open models (DeepSeek, Qwen, Llama, Nemotron). Press List models." },
      { name: "Hugging Face", url: "https://router.huggingface.co/v1", model: "", note: "A small monthly free credit with a Hugging Face token; routes to many open models. Press List models." },
      { name: "SambaNova", url: "https://api.sambanova.ai/v1", model: "", note: "Free tier at cloud.sambanova.ai with fast open models. Press List models." },
      { name: "Groq", url: "https://api.groq.com/openai/v1", model: "openai/gpt-oss-120b", note: "Free key at console.groq.com and very fast, but the free tier allows only a few thousand tokens a minute. Use a small context limit under Memory, or turns will be refused." },
      { name: "Cerebras", url: "https://api.cerebras.ai/v1", model: "gpt-oss-120b", note: "Free key at cloud.cerebras.ai and very fast, but the free tier's context window is short. Use a small context limit under Memory." },
    ]],
    ["Free on your own computer (needs a good graphics card)", [
      { name: "Ollama", url: "http://localhost:11434/v1", model: "", note: "ollama.com. Set its context length to 32k or more (OLLAMA_CONTEXT_LENGTH), or it forgets the campaign." },
      { name: "LM Studio", url: "http://localhost:1234/v1", model: "", note: "lmstudio.ai. Start the local server in the Developer tab, and load the model with a 32k+ context." },
      { name: "Jan", url: "http://localhost:1337/v1", model: "", note: "jan.ai. Turn on the local API server in Settings." },
      { name: "KoboldCpp", url: "http://localhost:5001/v1", model: "", note: "Popular for storytelling. Launch with --contextsize 32768 or more." },
      { name: "text-generation-webui", url: "http://localhost:5000/v1", model: "", note: "Start it with the --api flag." },
      { name: "llama.cpp / LocalAI", url: "http://localhost:8080/v1", model: "", note: "llama-server, or LocalAI. Start with -c 32768 or more." },
      { name: "vLLM", url: "http://localhost:8000/v1", model: "", note: "vllm serve <model>. Needs a large GPU." },
      { name: "GPT4All", url: "http://localhost:4891/v1", model: "", note: "Enable the local API server in its settings." },
    ]],
    ["Cheap paid", [
      { name: "DeepSeek", url: "https://api.deepseek.com/v1", model: "deepseek-v4-flash", note: "platform.deepseek.com. The regular V4 Flash: very cheap, quick, and good at storytelling. The key can also come from DEEPSEEK_API_KEY." },
      { name: "Xiaomi MiMo", url: "https://api.xiaomimimo.com/v1", model: "mimo-v2.6-pro", note: "platform.xiaomimimo.com. Token-plan subscribers use their regional address instead." },
    ]],
  ];
  const PRESETS = PRESET_GROUPS.flatMap(([, ps]) => ps);
  let presetName = $state(PRESETS.find((p) => p.url === cfg.other_url)?.name ?? "");
  let presetNote = $derived(PRESETS.find((p) => p.name === presetName)?.note ?? "");

  function preset(name: string) {
    presetName = name;
    const p = PRESETS.find((x) => x.name === name);
    if (!p) return;
    cfg.other_url = p.url;
    cfg.other_model = p.model;
    models = [];
  }

  let version = $state("");
  // Bumped after the Google client changes, so the account card checks again.
  let acctKey = $state(0);

  onMount(async () => {
    if (platform.web) return;
    claude = await call("claude_status");
    agy = await call("agy_status");
    version = await getVersion();
    if (agy.path) loadAgyModels();
  });

  async function loadAgyModels() {
    try {
      const live: [string, string][] = await call("agy_models");
      if (live.length) {
        const merged = [...AGY_MODELS];
        for (const [id, name] of live) {
          if (!merged.some((m) => m.id === id)) merged.push({ id, name, note: "" });
        }
        agyModels = merged;
      }
    } catch {}
  }

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

  async function installAgy() {
    installing = true;
    try {
      toast(await call("agy_install"));
      agy = await call("agy_status");
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
    <div class="row"><h2 class="grow" style="margin:0">Settings</h2>{#if version}<span class="small muted">Version {version}</span>{/if}<button onclick={() => { save(); close(); }}>Done</button></div>

    {#if playerOnly}
      <label class="field"><span>Your name</span><input bind:value={cfg.player_name} /></label>
      <h3>Music</h3>
      <Sound {cfg} />
      <h3>Read aloud</h3>
      <Voices {cfg} />
    {:else}
    <h3>Connect the GM</h3>
    <p class="small muted">The Play and Scribe tabs use this. Everything else works without it.</p>
    {#if platform.mobile}<p class="small muted">On a phone or tablet the GM needs an API key or another service. Claude Code and Antigravity run on computers only. The Scribe needs a service whose model can use tools, such as Gemini, DeepSeek or OpenRouter.</p>{:else}
    <label class="check"><input type="radio" bind:group={cfg.engine} value="claude_code" style="width:auto" /> <span>Claude Code: your Claude account and plan, no API key</span></label>{/if}
    <label class="check"><input type="radio" bind:group={cfg.engine} value="api" style="width:auto" /> <span>Anthropic API key: pay per use</span></label>
    {#if !platform.mobile}<label class="check"><input type="radio" bind:group={cfg.engine} value="antigravity" style="width:auto" /> <span>Antigravity: your Google account through the Antigravity CLI, no API key</span></label>{/if}
    <label class="check"><input type="radio" bind:group={cfg.engine} value="other" style="width:auto" /> <span>Other models: free ones (Gemini, OpenRouter, Mistral, GLM, local models…), DeepSeek, Xiaomi MiMo, and more</span></label>

    {#if cfg.engine === "claude_code"}
      <p class="small">{claude.path ? `✔ Found ${claude.path}` : "✖ Claude Code wasn't found."}</p>
      <div class="row">
        {#if !claude.path}<button class="primary" disabled={installing} onclick={install}>{installing ? "Installing…" : "Set up Claude Code for me"}</button>{/if}
        <button onclick={() => call("claude_sign_in").catch(toast)}>Sign in to Claude Code…</button>
      </div>
      <label class="field"><span>Its location, if it wasn't found</span><input bind:value={cfg.claude_path} /></label>
    {:else if cfg.engine === "api"}
      <label class="field"><span>API key</span><input type="password" bind:value={cfg.api_key} placeholder="sk-ant-…" /></label>
      <label class="check small"><input type="checkbox" bind:checked={cfg.remember_key} /> <span>Remember the key on this device</span></label>
      <label class="check small"><input type="checkbox" bind:checked={cfg.fallbacks} /> <span>If the model declines a request, retry it on a fallback model. Safety classifiers can occasionally flag ordinary fiction; this keeps the game going.</span></label>
    {:else if cfg.engine === "antigravity"}
      <p class="small">{agy.path ? `✔ Found ${agy.path}` : "✖ Antigravity wasn't found."}</p>
      <div class="row">
        {#if !agy.path}<button class="primary" disabled={installing} onclick={installAgy}>{installing ? "Installing…" : "Set up Antigravity for me"}</button>{/if}
        <button onclick={() => call("agy_sign_in").catch(toast)}>Sign in to Antigravity…</button>
      </div>
      <label class="field"><span>Model (who runs the GM)</span>
        <select bind:value={cfg.agy_model}>{#each agyModels as m}<option value={m.id}>{m.name}</option>{/each}</select>
      </label>
      <p class="small muted">{agyModels.find((m) => m.id === cfg.agy_model)?.note ?? ""}</p>
      <label class="field"><span>Its location, if it wasn't found</span><input bind:value={cfg.agy_path} /></label>
    {:else}
      <label class="field"><span>Service</span>
        <select value={presetName} onchange={(e) => preset(e.currentTarget.value)}>
          <option value="">Choose one, or type an address below…</option>
          {#each PRESET_GROUPS as [g, ps]}<optgroup label={g}>{#each ps as p}<option value={p.name}>{p.name}</option>{/each}</optgroup>{/each}
        </select>
      </label>
      {#if presetNote}<p class="small muted">{presetNote}</p>{/if}
      <label class="field"><span>Address</span><input bind:value={cfg.other_url} /></label>
      <label class="field"><span>API key (not needed for Ollama or LM Studio)</span><input type="password" bind:value={cfg.other_key} /></label>
      <label class="field"><span>Model</span><input bind:value={cfg.other_model} list="model-list" /></label>
      <datalist id="model-list">{#each models as m}<option value={m}></option>{/each}</datalist>
      <button onclick={fetchModels}>List models</button>
      <p class="small muted">Models on your own computer need a large context window (32,000 tokens or more). Smaller ones may forget details or the app's tags.</p>
    {/if}

    {#if cfg.engine === "claude_code" || cfg.engine === "api"}
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

    <h3>Account</h3>
    <p class="small muted">Sign in with Google to keep your stories in your own Google Drive and pick them up on any of your devices.</p>
    {#key acctKey}<Account list={false} />{/key}
    <details>
      <summary class="small">Google sign-in setup (advanced)</summary>
      <div class="col" style="margin-top:6px">
        <p class="small muted" style="margin:0">Signing in needs a Google "client": a free ID that tells Google which app is asking. Paste one here to turn sign-in on, or to use your own. Leave both blank to use the one this copy came with.</p>
        <p class="small" style="margin:0">To make one in the Google Cloud console (free, no card, about ten minutes):</p>
        <ol class="small" style="margin:0;padding-left:20px">
          <li>Sign in at <b>console.cloud.google.com</b>, make a project, then enable the <b>Google Drive API</b> in it.</li>
          <li>Left menu → <b>Google Auth Platform → Branding</b>: an app name and your email as support contact; user type <b>External</b>.</li>
          <li><b>Google Auth Platform → Data Access</b> → <b>Add or remove scopes</b>: tick <br /><code>https://www.googleapis.com/auth/drive.file</code> ("see, edit, create and delete only the specific Google Drive files you use with this app"), then save.</li>
          <li><b>Google Auth Platform → Audience → Test users</b>: add the Google account you'll sign in with. Without it the sign-in page says "Access blocked". While the app is in Testing, each sign-in stops working after <b>7 days</b>; <b>Publish app</b> (still free, no review needed for your own use) ends that.</li>
          <li><b>Google Auth Platform → Clients → Create client</b>: application type <b>TVs and Limited Input devices</b> — a "Desktop app", "Web application" or "Android" client can't sign in this way (that's the 401). Copy the <b>client ID</b> and <b>client secret</b> into the two boxes below. The phone uses this same client: the Android form that asks for a package name and SHA-1 is for a different kind of sign-in and isn't needed here.</li>
        </ol>
        <div class="row">
          <button class="ghost small" onclick={() => openUrl("https://console.cloud.google.com/flows/enableapi?apiid=drive.googleapis.com").catch(toast)}>Enable the Drive API</button>
          <button class="ghost small" onclick={() => openUrl("https://console.cloud.google.com/auth/overview").catch(toast)}>Open the Google Auth Platform</button>
        </div>
        <label class="field"><span>Google client ID</span><input bind:value={cfg.google_client_id} placeholder="1234-abcd.apps.googleusercontent.com" autocapitalize="off" autocomplete="off" spellcheck="false" /></label>
        <label class="field"><span>Google client secret</span><input type="password" bind:value={cfg.google_client_secret} autocomplete="off" /></label>
        <div class="row"><button onclick={async () => { await save(); acctKey++; }}>Save and check</button></div>
        <p class="small muted" style="margin:0">After that the Account card shows <b>Sign in with Google…</b>: press it, then type the code it shows at <b>google.com/device</b> in any browser. <code>drive.file</code> means the app only ever sees files it made itself, in its own "Campaign Keeper" folder.</p>
      </div>
    </details>

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
      <label class="check small"><input type="checkbox" bind:checked={cfg.ledger_auto} /> <span>Every 3 exchanges, have the Ledger check the status windows and suggest fixes (bottom-left of Play; nothing changes until you apply it)</span></label>
      <label class="check small"><input type="checkbox" bind:checked={cfg.tilemap_enabled} /> <span>Keep the picture map (Map tab) up to date: the helper model redraws it after each reply</span></label>
      <label class="check small"><input type="checkbox" bind:checked={cfg.continuity_check} /> <span>Check each reply against memory for contradictions (uses the helper once per turn)</span></label>
      <label class="check small"><input type="checkbox" bind:checked={cfg.playstyle_enabled} /> <span>Learn how I like to play, across campaigns</span></label>
    {/if}
    {/if}

    <h3>Art</h3>
    <label class="check"><input type="checkbox" bind:checked={cfg.pixel_art} /> <span>Turn the GM's pictures into pixel art</span></label>
    {#if cfg.pixel_art}
      <label class="field"><span>Size: {cfg.pixel_size} pixels wide, unless the GM picks one</span><input type="range" min="32" max="256" step="8" bind:value={cfg.pixel_size} /></label>
      <label class="field"><span>Colours: {cfg.pixel_colors}, unless the GM picks a palette</span><input type="range" min="2" max="64" bind:value={cfg.pixel_colors} /></label>
    {/if}

    <h3>Painted pictures</h3>
    <p class="small muted">An image model paints scenes for the GM, and the app turns them into pixel art if that's on. Only the host needs this. On this PC is free and private but slower, and needs the painter files installed. A service gets each picture's description and usually charges by the picture. With this off, the GM draws its own.</p>
    <label class="field"><span>Painter</span>
      <select bind:value={cfg.image_provider}>
        <option value="none">Off: the GM draws its own</option>
        {#if !platform.mobile}<option value="local">On this PC: SD-Turbo (free)</option>{/if}
        <option value="fal">fal.ai (FLUX Schnell)</option>
        <option value="openai">Another service (OpenAI-compatible images)</option>
      </select>
    </label>
    {#if cfg.image_provider === "fal" || cfg.image_provider === "openai"}
      <label class="field"><span>API key</span><input type="password" bind:value={cfg.image_key} /></label>
      {#if cfg.image_provider === "openai"}
        <label class="field"><span>Address</span><input bind:value={cfg.image_url} placeholder="https://api.together.xyz/v1" /></label>
        <label class="field"><span>Model</span><input bind:value={cfg.image_model} placeholder="black-forest-labs/FLUX.1-schnell" /></label>
      {/if}
    {/if}

    <h3>Appearance</h3>
    <label class="check"><input type="checkbox" bind:checked={cfg.dark_mode} onchange={save} /> <span>Dark mode</span></label>
    <label class="field"><span>Text size: {Math.round(cfg.ui_scale * 100)}%</span><input type="range" min="0.8" max="1.6" step="0.05" bind:value={cfg.ui_scale} onchange={save} /></label>
    <label class="field"><span>Reading speed: {cfg.tts_speed.toFixed(2)}x</span><input type="range" min="0.6" max="1.6" step="0.05" bind:value={cfg.tts_speed} /></label>
  </div>
</div>
