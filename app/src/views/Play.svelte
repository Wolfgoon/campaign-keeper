<script lang="ts">
  import { onMount, tick } from "svelte";
  import { writeText } from "../lib/clipboard";
  import { call, onGame, onTable, md, safeSvg, toast } from "../lib/api";
  import { music } from "../lib/music.svelte";
  import { speakEntry, stopSpeaking } from "../lib/voice.svelte";
  import Entry from "./Entry.svelte";
  import Status from "./Status.svelte";
  import Activity from "./Activity.svelte";
  import Table from "./Table.svelte";
  import Sound from "./Sound.svelte";
  import Voices from "./Voices.svelte";

  let { cfg, go } = $props();

  let entries = $state<any[]>([]);
  let start = $state(0);
  let stream = $state("");
  let status = $state("");
  let busy = $state(false);
  let info = $state<any>({});
  let rules = $state<any>(null);
  let input = $state("");
  let mode = $state("do");
  let side = $state("status");
  let table = $state<any>(null);
  let memoryLine = $state("");
  let pics = $state<any[]>([]);
  let tracks = $state<any[]>([]);
  let log: HTMLDivElement;
  let openActivity = $state<any>(null);
  let gmChat = $state(false);

  const MODES = [["do", "> Do"], ["say", '" Say'], ["story", "~ Story"]];
  const hasSave = $derived(!!info.saves);

  function nearBottom() {
    return !log || log.scrollHeight - log.scrollTop - log.clientHeight < 200;
  }
  async function toBottom() {
    await tick();
    if (log) log.scrollTop = log.scrollHeight;
  }

  async function refresh(forceBottom = false) {
    const stick = forceBottom || nearBottom();
    const r = await call("get_entries", { limit: Math.max(50, entries.length) });
    const before = entries.length ? entries[entries.length - 1]?.idx : -1;
    entries = r.entries;
    start = r.start;
    info = { ...(await call("play_state")), saves: info.saves };
    busy = info.busy;
    rules = await call("rpg_state");
    const last = entries[entries.length - 1];
    if (last && last.idx !== before && last.role === "assistant") onNewReply(last);
    if (stick) await toBottom();
  }

  function onNewReply(e: any) {
    if (e.music?.stop) music.stop();
    else if (e.music?.score) music.play(e.music);
    if (cfg.tts_auto) speakEntry(e.text, e.voices, cfg.tts_speed);
  }

  async function older() {
    const h = log.scrollHeight;
    const r = await call("get_entries", { before: start, limit: 50 });
    entries = [...r.entries, ...entries];
    start = r.start;
    await tick();
    log.scrollTop = log.scrollHeight - h;
  }

  function compose() {
    const t = input.trim();
    if (!t) return "";
    if (t.startsWith("/") || t.startsWith(">") || t.startsWith('"') || t.startsWith("~")) return t;
    if (mode === "say") return `"${t}"`;
    if (mode === "story") return `~ ${t}`;
    return `> ${t}`;
  }

  async function send(text?: string, label?: string) {
    const body = text ?? compose();
    if (!body) return;
    if (text === undefined) input = "";
    stopSpeaking();
    try {
      if (gmChat) await call("send_text", { text: body, private: true });
      else if (table && !body.startsWith("/")) {
        await call("table_host_action", { text: body, gmNarration: table.host_is_gm });
        toast("Locked in. Waiting for the table.");
      } else await call("send_text", { text: body, label });
      await refresh(true);
    } catch (e) {
      toast(e);
    }
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  }

  async function begin(how: string) {
    try {
      await call("begin", { how });
      await refresh(true);
    } catch (e) {
      toast(e);
    }
  }

  async function endChat() {
    if (!confirm("Archive this chat and go back to the start screen? Nothing is deleted.")) return;
    music.stop();
    await call("end_chat");
    entries = [];
    await refresh();
  }

  async function loadSide() {
    pics = await call("pictures");
    tracks = await call("tracks");
    table = await call("table_state");
    const o = await call("overview");
    info.saves = o.counts["Saves"];
  }

  async function copyGame() {
    await writeText(await call<string>("export_all"));
    toast("Copied the whole game. Paste it into a new chat on claude.ai.");
  }

  onMount(() => {
    refresh(true);
    loadSide();
    const a = onGame(async (kind, data) => {
      if (kind === "stream") {
        const stick = nearBottom();
        stream = data;
        if (stick) toBottom();
      } else if (kind === "status") status = data;
      else if (kind === "entry") await refresh(true);
      else if (kind === "done") {
        stream = "";
        status = "";
        await refresh();
        loadSide();
      } else if (kind === "error") toast(data);
      else if (kind === "note" || kind === "system") {
        if (kind === "system" && String(data).includes("\n")) entries.push({ idx: -entries.length - 1, role: "assistant", text: "```\n" + data + "\n```" });
        else toast(data);
      } else if (kind === "memory") memoryLine = data;
    });
    const b = onTable(async () => (table = await call("table_state")));
    return () => {
      a.then((f) => f());
      b.then((f) => f());
    };
  });

  /** Everyone else has acted, and the round is waiting on the host. */
  const waitingOnYou = $derived.by(() => {
    if (!table || busy) return false;
    const seats = (table.table?.seats ?? []).filter((s: any) => s.admitted && s.connected && s.character);
    const done = table.host_is_gm ? table.table?.narration : table.table?.host_action;
    return seats.length > 0 && !done && seats.every((s: any) => s.acted);
  });

  const pctContext = $derived(info.context_limit ? Math.min(100, (info.last_prompt_tokens / info.context_limit) * 100) : 0);
</script>

<div class="play">
  <div class="story">
    <div class="log" bind:this={log}>
      <div class="log-inner">
        {#if start > 0}<button class="ghost" onclick={older}>⬆ Show older</button>{/if}
        {#if !entries.length && !busy}
          <div class="card col" style="margin-top:8vh">
            <h2>Claude runs the game as your Game Master</h2>
            <p class="muted">It follows the campaign's instructions and everything in its files, and it remembers.</p>
            {#if hasSave}<button class="primary" onclick={() => begin("save")}>▶ Resume from the latest save</button>{/if}
            <button class:primary={!hasSave} onclick={() => begin("new")}>✨ Start a new game</button>
            <button onclick={() => begin("files")}>▶ Resume from the campaign files</button>
            <div class="row">
              <button class="ghost" onclick={copyGame}>📋 Copy for a chat elsewhere</button>
              <button class="ghost" onclick={() => (side = "table")}>👥 Play together</button>
            </div>
          </div>
        {/if}
        {#each entries as e (e.idx)}
          {#if !e.private || gmChat}<Entry {e} speak={(x: any) => speakEntry(x.text, x.voices, cfg.tts_speed)} />{/if}
        {/each}
        {#if stream}<div class="msg-gm">{@html md(stream)}</div>{/if}
        {#if busy && status && !stream}<div class="msg-note">{status}</div>{/if}
      </div>
    </div>
    <div class="composer">
      <div class="composer-inner">
        <div class="row modes" role="group" aria-label="Input mode">
          {#each MODES as [id, name]}
            <button class:on={mode === id} onclick={() => (mode = id)}>{name}</button>
          {/each}
          <span class="grow"></span>
          {#if table?.host_is_gm}<label class="row small"><input type="checkbox" bind:checked={gmChat} /> 🔒 GM chat</label>{/if}
          <button class="ghost small" disabled={busy || !entries.length} title="Discard the GM's last reply and get a new one. Checks stand." onclick={() => send("/retry")}>↻ Retry</button>
          <button class="ghost small" disabled={busy || !entries.length} title="Remove your last message and the GM's reply" onclick={() => send("/undo")}>↺ Undo</button>
          <button class="ghost small" disabled={busy} onclick={() => send("/continue")}>Continue</button>
          <button class="ghost small" disabled={busy} onclick={() => send("/scene")}>Scene</button>
          <button class="ghost small" disabled={busy} onclick={() => send("/save")}>💾 /save</button>
          <button class="ghost small" onclick={() => go("scribe")}>✏ Scribe</button>
          {#if busy}<button class="small danger" onclick={() => call("stop_turn")}>⏹ Stop</button>{/if}
        </div>
        {#if waitingOnYou}
          <div class="row small" role="status">
            <span class="grow" style="color:var(--gold)">Everyone else is ready. The round is waiting for you.</span>
            <button onclick={() => call("table_send_now").then(() => loadSide()).catch(toast)}>▶ Send the round now</button>
          </div>
        {/if}
        <textarea rows="2" bind:value={input} onkeydown={keydown} placeholder={'What do you do?   > act   "speak"   ~ narrate   /help'} aria-label="Your action"></textarea>
        <div class="row small muted">
          <div class="gauge" class:high={pctContext > 85} title="Context used before older turns get condensed"><i style:width="{pctContext}%"></i></div>
          <span>{Math.round((info.last_prompt_tokens ?? 0) / 1000)}k / {Math.round((info.context_limit ?? 0) / 1000)}k</span>
          {#if info.condensed?.times}<span>· condensed {info.condensed.times}×</span>{/if}
          {#if info.cost}<span>· ≈ ${info.cost.toFixed(2)}</span>{/if}
          <span class="grow" style="text-align:right">{memoryLine}</span>
          {#if entries.length}<button class="ghost small" onclick={endChat}>End chat…</button>{/if}
        </div>
      </div>
    </div>
  </div>

  <aside class="side">
    <div class="side-tabs">
      {#each [["status", "Status"], ["pics", "🖼"], ["music", "🎵"], ["voices", "💬"], ["table", "👥"]] as [id, name]}
        <button class="tab" class:on={side === id} aria-label={id} onclick={() => { side = id; loadSide(); }}>{name}</button>
      {/each}
    </div>
    <div class="side-body">
      {#if side === "status"}
        <Status bind:rules />
      {:else if side === "pics"}
        {#if !pics.length}<p class="small muted">None yet. The GM draws maps, items and characters when they matter to the story.</p>{/if}
        {#each pics as p}
          <figure class="pic" style="margin-inline:0">{@html safeSvg(p.svg)}<figcaption>{p.title}</figcaption></figure>
        {/each}
      {:else if side === "music"}
        <Sound {cfg} />
        {#if !tracks.length}<p class="small muted">The GM's tracks appear here once it composes some.</p>{/if}
        {#each tracks as t}
          <div class="track">
            <button class="ghost" aria-label="Play {t.title}" onclick={() => music.play(t)}>▶</button>
            <span class="grow">{t.title}</span>
          </div>
        {/each}
      {:else if side === "voices"}
        <Voices {cfg} />
      {:else}
        <Table bind:table {cfg} />
      {/if}
    </div>
  </aside>
</div>

{#each info.activities ?? [] as a (a.id)}
  {#if !openActivity}
    <button class="toast" onclick={() => (openActivity = a)}>{a.icon ?? "✍"} {a.title ?? "An activity is open"}</button>
  {/if}
{/each}
{#if openActivity}
  <Activity a={openActivity}
    submit={async (text: string, id: number) => { openActivity = null; await call("activity_close", { id }); await send(text); }}
    close={async (id: number) => { openActivity = null; await call("activity_close", { id }); await refresh(); }} />
{/if}
