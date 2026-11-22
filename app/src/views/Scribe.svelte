<script lang="ts">
  import { onMount } from "svelte";
  import { call, md, toast } from "../lib/api";
  import { platform } from "../lib/platform.svelte";

  // The Scribe edits files through the model's tools, so it can't run on every engine.
  let { cfg } = $props();

  let presets = $state<any>({});
  let request = $state("");
  let includeChat = $state(true);
  let working = $state(false);
  let result = $state<any>(null);
  let picked = $state<Record<string, boolean>>({});
  let open = $state<Record<string, boolean>>({});
  let lastBackup = $state("");

  // Who will do the editing, in the player's terms, and whether there's anyone to ask yet.
  const brain = $derived.by(() => {
    if (!cfg) return { name: "", ready: false, note: "" };
    if (cfg.engine === "api") {
      return cfg.api_key?.trim()
        ? { name: "your Anthropic key", ready: true, note: "" }
        : { name: "your Anthropic key", ready: false, note: "Add your API key in Settings, or pick another service there." };
    }
    if (cfg.engine === "other") {
      const model = String(cfg.other_model ?? "").trim();
      return model
        ? { name: model, ready: true, note: "" }
        : { name: "another service", ready: false, note: "Pick a service and a model in Settings → Other models. It has to be one that can use tools: the Gemini, DeepSeek and OpenRouter keys can." };
    }
    return { name: cfg.engine === "antigravity" ? "Antigravity" : "Claude Code", ready: true, note: "" };
  });

  onMount(async () => {
    presets = await call("scribe_presets");
    request = presets.after;
  });

  async function run() {
    working = true;
    result = null;
    try {
      result = await call("scribe_run", { request, includeChat });
      picked = Object.fromEntries(result.proposals.map((p: any) => [p.path, true]));
    } catch (e) {
      toast(e);
    }
    working = false;
  }

  async function apply() {
    const approved = result.proposals.filter((p: any) => picked[p.path]).map((p: any) => [p.path, p.new]);
    lastBackup = await call("scribe_apply", { approved });
    toast(`Wrote ${approved.length} files. Old versions are in the backups.`);
    result = null;
  }

  async function restore() {
    await call("scribe_restore", { backup: lastBackup });
    lastBackup = "";
    toast("Restored the files from before the Scribe's changes.");
  }
</script>

<div class="pad scroll grow" style="max-width:900px;margin:0 auto">
  <h1>Scribe</h1>
  <p class="muted">The model reads the campaign files and the play session, and proposes edits. Nothing is written until you approve it.</p>
  <p class="small muted">
    Edits are made by {brain.name}.
    {#if platform.mobile}This works on the phone: it uses the model from Settings, not a program on a computer.{/if}
    {#if working}It can take a few minutes on a free service.{/if}
  </p>
  {#if !brain.ready}<p class="small error">{brain.note}</p>{/if}
  <div class="row" style="margin-bottom:8px">
    <button onclick={() => (request = presets.after)}>After-session checklist</button>
    <button onclick={() => (request = presets.log)}>Just the session log</button>
    <button onclick={() => (request = presets.check)}>Check for contradictions</button>
  </div>
  <label class="field"><span>What should the Scribe do?</span><textarea rows="5" bind:value={request}></textarea></label>
  <label class="check" style="margin:8px 0"><input type="checkbox" bind:checked={includeChat} /> <span>Include this play session</span></label>
  <div class="row">
    <button class="primary" disabled={working || !request.trim() || !brain.ready} onclick={run}>{working ? "Working…" : "✏ Run the Scribe"}</button>
    {#if lastBackup}<button onclick={restore}>Restore the backups and remove the new files</button>{/if}
  </div>

  {#if result}
    <div class="card" style="margin-top:14px">{@html md(result.summary || "Done.")}</div>
    {#if !result.proposals.length}
      <p class="muted" style="margin-top:10px">No file changes were proposed.</p>
    {:else}
      <div class="row" style="margin:12px 0 6px">
        <h2 class="grow" style="margin:0">{result.proposals.length} proposed changes</h2>
        <button class="primary" onclick={apply}>Approve selected</button>
        <button onclick={() => (result = null)}>Discard all</button>
      </div>
      {#each result.proposals as p}
        <div class="card" style="margin-bottom:8px">
          <div class="row">
            <label class="row grow"><input type="checkbox" bind:checked={picked[p.path]} /> <b>{p.path}</b> {#if p.is_new}<span class="small" style="color:var(--gold)">NEW</span>{/if}</label>
            <button class="ghost small" onclick={() => (open[p.path] = !open[p.path])}>{open[p.path] ? "Hide changes" : "Show changes"}</button>
          </div>
          {#if open[p.path]}
            <div class="diff" style="margin-top:8px">
              {#each p.lines as l}<div class={l.tag}>{l.tag === "add" ? "+ " : l.tag === "del" ? "- " : l.tag === "gap" ? "" : "  "}{l.text}</div>{/each}
            </div>
          {/if}
        </div>
      {/each}
    {/if}
  {/if}
</div>
