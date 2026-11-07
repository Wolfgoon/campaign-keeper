<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { call, bindToast, toast } from "./lib/api";
  import { music } from "./lib/music.svelte";
  import Welcome from "./views/Welcome.svelte";
  import Overview from "./views/Overview.svelte";
  import Play from "./views/Play.svelte";
  import Files from "./views/Files.svelte";
  import Scribe from "./views/Scribe.svelte";
  import Search from "./views/Search.svelte";
  import Memory from "./views/Memory.svelte";
  import Settings from "./views/Settings.svelte";
  import Client from "./views/Client.svelte";

  let cfg = $state<any>(null);
  let campaign = $state<{ path: string; name: string } | null>(null);
  let tab = $state("overview");
  let joined = $state(false);
  let settingsOpen = $state(false);
  let toastText = $state("");
  let toastTimer: any;

  const TABS = [
    ["overview", "🏠 Overview"], ["play", "▶ Play"], ["files", "📁 Files"], ["scribe", "✏ Scribe"], ["search", "🔍 Search"], ["memory", "🧠 Memory"],
  ];

  bindToast((s) => {
    toastText = s;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toastText = ""), 4500);
  });

  function applyTheme() {
    if (!cfg) return;
    document.documentElement.dataset.theme = cfg.dark_mode ? "dark" : "light";
    document.documentElement.style.zoom = String(cfg.ui_scale || 1);
    music.setVolume(cfg.music_volume ?? 0.5, cfg.music_muted ?? false);
  }

  async function saveCfg() {
    await call("set_config", { cfg: $state.snapshot(cfg) });
    applyTheme();
  }

  async function open(path: string) {
    try {
      campaign = await call("open_campaign", { path });
      tab = "overview";
    } catch (e) {
      toast(e);
    }
  }

  async function close() {
    music.stop();
    await call("close_campaign");
    campaign = null;
    cfg = await call("get_config");
  }

  onMount(() => {
    (async () => {
      cfg = await call("get_config");
      applyTheme();
    })();
    const un = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "drop" && e.payload.paths[0]) open(e.payload.paths[0]);
    });
    return () => void un.then((f) => f());
  });
</script>

<div class="shell">
  {#if joined}
    <Client {cfg} leave={() => (joined = false)} settings={() => (settingsOpen = true)} />
  {:else if !campaign}
    {#if cfg}
      <div class="view">
        <Welcome {cfg} {open} created={(c: any) => { campaign = c; tab = "play"; }} join={() => (joined = true)} settings={() => (settingsOpen = true)} />
      </div>
    {/if}
  {:else}
    <nav class="topbar" aria-label="Campaign">
      <span class="title" title={campaign.path}>{campaign.name}</span>
      {#each TABS as [id, name]}
        <button class="tab" class:on={tab === id} onclick={() => (tab = id)}>{name}</button>
      {/each}
      <span class="grow"></span>
      {#if music.title}
        <span class="small muted">🎵 {music.title}</span>
        <button class="ghost small" onclick={() => music.stop()}>Stop</button>
      {/if}
      <button class="ghost" onclick={() => (settingsOpen = true)} aria-label="Settings">⚙</button>
      <button class="ghost" onclick={close}>Close</button>
    </nav>
    <div class="view">
      <!-- Play stays mounted so a reply keeps streaming while you read a file. -->
      <div class="view" style:display={tab === "play" ? "flex" : "none"}><Play {cfg} go={(t: string) => (tab = t)} /></div>
      {#if tab === "overview"}<Overview go={(t: string) => (tab = t)} />
      {:else if tab === "files"}<Files />
      {:else if tab === "scribe"}<Scribe />
      {:else if tab === "search"}<Search />
      {:else if tab === "memory"}<Memory />
      {/if}
    </div>
  {/if}
</div>

{#if settingsOpen && cfg}
  <Settings {cfg} save={saveCfg} close={() => (settingsOpen = false)} playerOnly={joined} />
{/if}
{#if toastText}<div class="toast" role="status">{toastText}</div>{/if}
