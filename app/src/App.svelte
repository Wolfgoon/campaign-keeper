<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { listen } from "@tauri-apps/api/event";
  import { call, bindToast, toast, inApp } from "./lib/api";
  import { platform, loadPlatform } from "./lib/platform.svelte";
  import { music } from "./lib/music.svelte";
  import Welcome from "./views/Welcome.svelte";
  import Overview from "./views/Overview.svelte";
  import Play from "./views/Play.svelte";
  import Party from "./views/Party.svelte";
  import Host from "./views/Host.svelte";
  import Settings from "./views/Settings.svelte";
  import Client from "./views/Client.svelte";

  let cfg = $state<any>(null);
  let campaign = $state<{ path: string; name: string } | null>(null);
  let tab = $state("overview");
  let joined = $state(false);
  let settingsOpen = $state(false);
  let toastText = $state("");
  let toastTimer: any;
  // Account sync: "" (signed out), working, ok, conflict or error.
  let syncState = $state("");
  let syncNote = $state("");
  let conflict = $state<any>(null);
  let askConflict = $state(false);
  let opening = $state(false);
  // Bumped when the account's copy replaces the open game, so every view reads it afresh.
  let epoch = $state(0);

  // The story, the party and their notes are the whole table's. Host-only tools sit behind one tab.
  const TABS = [
    ["overview", "🏠 Overview"], ["play", "▶ Play"], ["party", "👥 Party"], ["host", "🛠 Host"],
  ];

  // Where the Host tab was last left, so "Scribe" in Play opens the Scribe, not the table.
  let hostPart = $state("table");

  /** Move to a tab; "host:scribe" opens the Host tab on one of its pages. */
  function go(t: string) {
    if (t.startsWith("host:")) {
      hostPart = t.slice(5);
      tab = "host";
    } else {
      tab = t;
    }
  }

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

  function took(v: any) {
    if (v.state === "off") syncState = "";
    else if (v.state === "error") {
      syncState = "error";
      syncNote = v.error;
    } else if (v.state === "conflict") {
      syncState = "conflict";
      conflict = v.cloud;
    } else {
      syncState = "ok";
      conflict = null;
      syncNote = "";
      if (v.state === "downloaded") {
        epoch++;
        toast("Loaded the newer version of this story from your account.");
      }
    }
  }

  async function sync(prefer?: string) {
    if (syncState === "working") return;
    syncState = "working";
    try {
      took(await call("sync_now", { prefer }));
    } catch (e) {
      took({ state: "error", error: String(e) });
    }
  }

  async function settle(prefer: string) {
    askConflict = false;
    await sync(prefer);
    if (syncState === "error") toast(syncNote);
  }

  async function open(path: string) {
    if (opening) return;
    opening = true;
    try {
      const c = await call<any>("open_campaign", { path });
      // Before the game shows: another device may have played further.
      syncState = "";
      await sync();
      askConflict = syncState === "conflict";
      campaign = c;
      tab = "overview";
    } catch (e) {
      toast(e);
    }
    opening = false;
  }

  async function close() {
    music.stop();
    if (syncState === "ok" || syncState === "error") await sync();
    syncState = "";
    await call("close_campaign");
    campaign = null;
    cfg = await call("get_config");
  }

  onMount(() => {
    let un: Promise<() => void> | undefined;
    (async () => {
      await loadPlatform();
      // In a friend's browser the page is only ever the player's screen.
      joined = platform.web;
      cfg = await call("get_config");
      applyTheme();
      if (inApp && !platform.mobile) {
        un = getCurrentWebview().onDragDropEvent((e) => {
          if (e.payload.type === "drop" && e.payload.paths[0]) open(e.payload.paths[0]);
        });
      }
    })();
    const sy = inApp
      ? listen<any>("sync", (e) => {
          if (campaign && syncState !== "working") took(e.payload);
        })
      : Promise.resolve(() => {});
    return () => {
      un?.then((f) => f());
      sy.then((f) => f());
    };
  });
</script>

<div class="shell">
  {#if joined && cfg}
    <Client {cfg} leave={() => (joined = platform.web)} settings={() => (settingsOpen = true)} />
  {:else if !campaign}
    {#if cfg}
      <div class="view">
        <Welcome {cfg} {open} created={(c: any) => { campaign = c; tab = "play"; syncState = ""; sync(); }} join={() => (joined = true)} settings={() => (settingsOpen = true)} />
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
      {#if syncState}
        <button class="ghost small" class:error={syncState === "conflict" || syncState === "error"}
          title={syncState === "error" ? syncNote : "Your account keeps this story in step on your devices. Click to sync now."}
          onclick={() => (syncState === "conflict" ? (askConflict = true) : sync().then(() => syncState === "error" && toast(syncNote)))}>
          ☁ {syncState === "working" ? "Syncing…" : syncState === "ok" ? "Synced" : syncState === "conflict" ? "Choose a version" : "Not synced"}
        </button>
      {/if}
      <button class="ghost" onclick={() => (settingsOpen = true)} aria-label="Settings">⚙</button>
      <button class="ghost" onclick={close}>Close</button>
    </nav>
    {#key epoch}
    <div class="view">
      <!-- Play stays mounted so a reply keeps streaming while you read a file. -->
      <div class="view" style:display={tab === "play" ? "flex" : "none"}><Play {cfg} save={saveCfg} {go} /></div>
      {#if tab === "overview"}<Overview {go} />
      {:else if tab === "party"}<Party {go} />
      {:else if tab === "host"}<Host {cfg} bind:part={hostPart} />
      {/if}
    </div>
    {/key}
  {/if}
</div>

{#if opening}<div class="toast" role="status">Opening…</div>{/if}
{#if askConflict && conflict}
  <div class="modal-back">
    <div class="card col modal">
      <h2>This story changed in two places</h2>
      <p>It was played or edited on this device, and your account has a different version{conflict.device ? ` saved from ${conflict.device}` : ""}{conflict.modified ? ` on ${new Date(conflict.modified).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" })}` : ""}. Which one do you want to keep?</p>
      <button class="primary" onclick={() => settle("local")}>Keep this device's version</button>
      <button onclick={() => settle("cloud")}>Use the account's version</button>
      <button class="ghost" onclick={() => (askConflict = false)}>Decide later</button>
      <p class="small muted">If you use the account's version, this device's files are first saved in a backup on this device.</p>
    </div>
  </div>
{/if}

{#if settingsOpen && cfg}
  <Settings {cfg} save={saveCfg} close={() => (settingsOpen = false)} playerOnly={joined} />
{/if}
{#if toastText}<div class="toast" role="status">{toastText}</div>{/if}
