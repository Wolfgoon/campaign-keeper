<script lang="ts">
  import { onMount } from "svelte";
  import { open as pick } from "@tauri-apps/plugin-dialog";
  import { call, toast } from "../lib/api";
  import { platform } from "../lib/platform.svelte";
  import Account from "./Account.svelte";

  let { cfg, open, created, join, settings } = $props();
  let making = $state(false);
  let title = $state("");
  // The campaigns on this device, read from disk. The recent list can only name what was opened
  // in a window that still remembers it; this is everything that is actually here.
  let found = $state<any[]>([]);
  let packing = $state(false);

  onMount(async () => {
    try {
      found = await call<any[]>("list_campaigns");
    } catch {
      found = [];
    }
  });

  // A story opened from somewhere else (a folder on a PC) keeps its place in the recent list.
  let elsewhere = $derived((cfg.recent ?? []).filter((p: string) => !found.some((c) => c.path === p)));

  async function browse() {
    const p = await pick({ directory: true, title: "Open a campaign folder" });
    if (typeof p === "string") open(p);
  }

  async function create() {
    // A phone keeps every campaign in the app's own storage: there's no folder to choose.
    const parent = platform.mobile ? "" : await pick({ directory: true, title: "Where should the campaign folder go?" });
    if (typeof parent !== "string") return;
    try {
      created(await call("new_campaign", { parent, title }));
    } catch (e) {
      toast(e);
    }
  }

  /** A campaign's badge: its first letter on a colour of its own. */
  function initial(n: string) {
    return ([...String(n ?? "?").trim()][0] ?? "?").toUpperCase();
  }
  function hue(n: string) {
    let h = 0;
    for (const c of String(n)) h = (h * 31 + c.charCodeAt(0)) % 360;
    return `hsl(${h} 45% 38%)`;
  }

  function shortName(p: string) {
    return p.split(/[\\/]/).pop();
  }

  /** When a campaign was last played, in the fewest words that still say it. */
  function since(sec: number) {
    const days = Math.floor((Date.now() - sec * 1000) / 86400000);
    if (days <= 0) return "played today";
    if (days === 1) return "played yesterday";
    if (days < 7) return `played ${days} days ago`;
    return `played ${new Date(sec * 1000).toLocaleDateString()}`;
  }

  function played(c: any) {
    const saves = c.saves === 1 ? "1 save" : `${c.saves} saves`;
    return c.modified ? `${saves} · ${since(c.modified)}` : saves;
  }

  async function forget(p: string) {
    cfg.recent = cfg.recent.filter((x: string) => x !== p);
    await call("forget_recent", { path: p });
  }

  /** One zip of every campaign, for a computer: a phone's own storage can't be browsed. */
  async function saveCopy() {
    packing = true;
    try {
      toast(`Saved a copy: ${await call<string>("export_campaigns")}`);
    } catch (e) {
      toast(e);
    }
    packing = false;
  }
</script>

<div class="welcome">
  <section class="hero">
    <img src="/icon.png" alt="" />
    <div>
      <h1>Campaign Keeper</h1>
      <p class="muted">Keep your campaigns organized, and play them with Claude as your Game Master.</p>
    </div>
    {#if making}
      <div class="card col">
        <label class="field"><span>Campaign name</span><input bind:value={title} placeholder="e.g. Second Light" /></label>
        <div class="row">
          <button class="primary" disabled={!title.trim()} onclick={create}>{platform.mobile ? "Create it" : "Choose where to put it…"}</button>
          <button onclick={() => (making = false)}>Cancel</button>
        </div>
      </div>
    {:else}
      <button class="primary big" onclick={() => (making = true)}>✨ Start a new campaign…</button>
      {#if !platform.mobile}<button onclick={browse}>📂 Open a campaign folder…</button>{/if}
      <button onclick={join}>👥 Join a friend's game…<span class="muted small" style="display:block">No Claude account needed</span></button>
    {/if}
    {#if !platform.mobile}<p class="small muted drop">Or drop a campaign folder anywhere on this window.</p>{/if}
    <span class="grow"></span>
    <div class="row">
      {#if found.length}
        <button class="ghost small" disabled={packing} onclick={saveCopy}>💾 {packing ? "Saving a copy…" : "Back up every campaign (zip)"}</button>
      {/if}
      <span class="grow"></span>
      <button class="ghost small" onclick={settings}>⚙ Settings</button>
    </div>
  </section>

  <section class="library">
    {#if found.length}
      <h2>Your campaigns</h2>
      <div class="shelf">
        {#each found as c (c.path)}
          <button class="camp" title={c.path} onclick={() => open(c.path)}>
            <span class="badge" style:background={hue(c.name)}>{initial(c.name)}</span>
            <span class="col" style="gap:2px;min-width:0">
              <b class="camp-name">{c.name}</b>
              <span class="muted small">{played(c)}</span>
            </span>
            <span class="go" aria-hidden="true">▶</span>
          </button>
        {/each}
      </div>
    {:else}
      <div class="empty">
        <h2>No campaigns yet</h2>
        <p class="muted">Start a new one on the left, or open a campaign folder you already have.</p>
      </div>
    {/if}
    {#if elsewhere.length}
      <h3>{found.length ? "Also opened" : "Recent"}</h3>
      <div class="col" style="gap:4px">
        {#each elsewhere as r (r)}
          <div class="recent">
            <button title={r} onclick={() => open(r)}>📁 {shortName(r)} <span class="muted small">{r}</span></button>
            <button class="ghost" aria-label="Remove from this list" onclick={() => forget(r)}>✖</button>
          </div>
        {/each}
      </div>
    {/if}
    <div class="account-box"><Account {open} /></div>
  </section>
</div>
