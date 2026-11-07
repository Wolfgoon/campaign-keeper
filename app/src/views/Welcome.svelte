<script lang="ts">
  import { open as pick } from "@tauri-apps/plugin-dialog";
  import { call, toast } from "../lib/api";

  let { cfg, open, created, join, settings } = $props();
  let making = $state(false);
  let title = $state("");

  async function browse() {
    const p = await pick({ directory: true, title: "Open a campaign folder" });
    if (typeof p === "string") open(p);
  }

  async function create() {
    const parent = await pick({ directory: true, title: "Where should the campaign folder go?" });
    if (typeof parent !== "string") return;
    try {
      created(await call("new_campaign", { parent, title }));
    } catch (e) {
      toast(e);
    }
  }

  function shortName(p: string) {
    return p.split(/[\\/]/).pop();
  }
</script>

<div class="welcome">
  <img src="/icon.png" alt="" />
  <div>
    <h1>Campaign Keeper</h1>
    <p class="muted">Keep your campaigns organized, and play them with Claude as your Game Master. Drop a campaign folder here to open it.</p>
  </div>
  {#if making}
    <div class="card col">
      <label class="field"><span>Campaign name</span><input bind:value={title} placeholder="e.g. Second Light" /></label>
      <div class="row">
        <button class="primary" disabled={!title.trim()} onclick={create}>Choose where to put it…</button>
        <button onclick={() => (making = false)}>Cancel</button>
      </div>
    </div>
  {:else}
    <button class="primary" onclick={() => (making = true)}>✨ Start a new campaign…</button>
    <button onclick={browse}>📂 Open a campaign folder…</button>
    <button onclick={join}>👥 Join a friend's game… <span class="muted small">no Claude account needed</span></button>
  {/if}
  {#if cfg.recent?.length}
    <h3 style="margin-top:6px">Recent</h3>
    {#each cfg.recent as r}
      <div class="recent">
        <button title={r} onclick={() => open(r)}>{shortName(r)}</button>
        <button class="ghost" aria-label="Remove from this list" onclick={async () => { cfg.recent = cfg.recent.filter((x: string) => x !== r); await call("set_config", { cfg: $state.snapshot(cfg) }); }}>✖</button>
      </div>
    {/each}
  {/if}
  <button class="ghost" onclick={settings}>⚙ Settings</button>
</div>
