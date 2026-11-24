<script lang="ts">
  import { onMount, tick } from "svelte";
  import { writeText } from "../lib/clipboard";
  import { call, onGame, onTable, md, toast, MODELS, AGY_MODELS } from "../lib/api";
  import { music } from "../lib/music.svelte";
  import { platform } from "../lib/platform.svelte";
  import Art from "./Art.svelte";
  import Maps from "./Maps.svelte";
  import MapTab from "./MapTab.svelte";
  import LedgerDock from "./LedgerDock.svelte";
  import { speakEntry, stopSpeaking } from "../lib/voice.svelte";
  import Entry from "./Entry.svelte";
  import Status from "./Status.svelte";
  import Activity from "./Activity.svelte";
  import Sound from "./Sound.svelte";
  import Voices from "./Voices.svelte";

  let { cfg, save, go } = $props();

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
  let maps = $state<any[]>([]);
  let tracks = $state<any[]>([]);
  let log: HTMLDivElement;
  let openActivity = $state<any>(null);
  let gmChat = $state(false);
  // On a narrow screen the side panel is a full-screen sheet, opened from the composer.
  let sideOpen = $state(false);

  const MODES = [["do", "> Do"], ["say", '" Say'], ["story", "~ Story"], ["ooc", "? OOC"]];
  // Out of character: the GM answers plainly and the story stays exactly where it was. These fill
  // the box rather than send, so nothing goes that the player hasn't read.
  const OOC_ASKS = [
    ["✔ Confirm", "Confirm what's true right now, out of character: what has definitely happened, what we know for certain, and what is still unclear. Don't continue the story."],
    ["🔍 More detail", "More detail, please, out of character: what's in front of me, what I can see, hear, smell and touch, who is here and what they are doing, and anything I could use. Be concrete and specific, and keep it consistent with what is already written."],
    ["🧷 No retcons", "Out of character: check this chat and the campaign files for anything that contradicts what we have already established. Nothing that has happened may be changed, softened or rewritten. If something is wrong, tell me plainly instead of narrating it away."],
    ["📒 Books", "Bookkeeping, out of character: go through what has happened and bring my status up to date: inventory (items gained, used, sold, lost), money, health and conditions. Change the status window to match, then list what you changed. No new events. Also: "],
  ];
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
    // An answer to an out-of-character question is not story: nothing to score, nothing to read out.
    if (e.ooc) return;
    music.apply(e);
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
    if (t.startsWith("/") || t.startsWith(">") || t.startsWith('"') || t.startsWith("~") || t.startsWith("?")) return t;
    if (mode === "ooc") return `? ${t}`;
    if (mode === "say") return `"${t}"`;
    if (mode === "story") return `~ ${t}`;
    return `> ${t}`;
  }

  async function send(text?: string, label?: string) {
    const body = text ?? compose();
    if (!body) return;
    if (text === undefined) input = "";
    stopSpeaking();
    // A question asked out of character goes to the GM alone: it isn't a move at the table, so it
    // never joins the round.
    const ooc = body.startsWith("?") || body.startsWith("/ooc");
    try {
      if (gmChat) await call("send_text", { text: body, private: true });
      else if (table && !ooc && !body.startsWith("/")) {
        await call("table_host_action", { text: body, gmNarration: table.host_is_gm });
        toast("Locked in. Waiting for the table.");
      } else await call("send_text", { text: body, label });
      await refresh(true);
    } catch (e) {
      toast(e);
    }
  }

  function keydown(e: KeyboardEvent) {
    // A phone keyboard's Enter makes a new line; the Send button sends.
    if (e.key === "Enter" && !e.shiftKey && !platform.mobile) {
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
    maps = await call("maps");
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
  // A host playing a character sees that character's sheet, exactly as their players do.
  const hostPlays = $derived<string | null>(table && !table.host_is_gm ? (table.host_character ?? "") : null);
</script>

<div class="play">
  <div class="story">
    <div class="log" bind:this={log}>
      <div class="log-inner">
        {#if start > 0}<button class="ghost" onclick={older}>⬆ Show older</button>{/if}
        {#if !entries.length && !busy}
          <div class="card col" style="margin-top:8vh">
            <h2>{cfg.engine === "antigravity" ? "Antigravity" : cfg.engine === "other" ? (cfg.other_model || "Your chosen model") : "Claude"} runs the game as your Game Master</h2>
            <p class="muted">It follows the campaign's instructions and everything in its files, and it remembers.</p>
            {#if hasSave}<button class="primary" onclick={() => begin("save")}>▶ Resume from the latest save</button>{/if}
            <button class:primary={!hasSave} onclick={() => begin("new")}>✨ Start a new game</button>
            <button onclick={() => begin("files")}>▶ Resume from the campaign files</button>
            <div class="row">
              <button class="ghost" onclick={copyGame}>📋 Copy for a chat elsewhere</button>
              <button class="ghost" onclick={() => go("host")}>👥 Play together</button>
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
            <button
              class:on={mode === id}
              title={id === "ooc" ? "Ask out of character: confirm what's true, ask for more detail, or say what may not change" : undefined}
              onclick={() => (mode = id)}>{name}</button>
          {/each}
          <span class="grow"></span>
          {#if table?.host_is_gm}<label class="row small"><input type="checkbox" bind:checked={gmChat} /> 🔒 GM chat</label>{/if}
          <button class="ghost small" disabled={busy || !entries.length} title="Discard the GM's last reply and get a new one. Checks stand." onclick={() => send("/retry")}>↻ Retry</button>
          <button class="ghost small" disabled={busy || !entries.length} title="Remove your last message and the GM's reply" onclick={() => send("/undo")}>↺ Undo</button>
          <button class="ghost small" disabled={busy} onclick={() => send("/continue")}>Continue</button>
          <button class="ghost small" disabled={busy} onclick={() => send("/scene")}>Scene</button>
          <button class="ghost small" disabled={busy} onclick={() => send("/save")}>💾 /save</button>
          <button class="ghost small" onclick={() => go("party")}>👥 Party</button>
          <button class="ghost small" onclick={() => go("host:scribe")}>✏ Scribe</button>
          <button class="ghost small" title="Fix inventory, money and health outside the story" onclick={() => go("host:ledger")}>📒 Ledger</button>
          <button class="small narrow-only" onclick={() => { sideOpen = true; loadSide(); }}>☰ Status, map, music…</button>
          {#if busy}<button class="small danger" onclick={() => call("stop_turn")}>⏹ Stop</button>{/if}
        </div>
        {#if mode === "ooc"}
          <div class="row" role="group" aria-label="Out-of-character asks">
            <span class="small muted">Out of character:</span>
            {#each OOC_ASKS as [name, ask]}
              <button class="small" title={ask} onclick={() => (input = ask)}>{name}</button>
            {/each}
          </div>
        {/if}
        {#if waitingOnYou}
          <div class="row small" role="status">
            <span class="grow" style="color:var(--gold)">Everyone else is ready. The round is waiting for you.</span>
            <button onclick={() => call("table_send_now").then(() => loadSide()).catch(toast)}>▶ Send the round now</button>
          </div>
        {/if}
        <div class="send-row">
          <textarea rows="2" bind:value={input} onkeydown={keydown} placeholder={mode === "ooc" ? "Ask out of character: confirm, more detail, no retcons" : 'What do you do?   > act   "speak"   ~ narrate   ? ask   /help'} aria-label="Your action"></textarea>
          <button class="primary narrow-only" disabled={!input.trim()} onclick={() => send()}>Send</button>
        </div>
        <div class="row small muted">
          <div class="gauge" class:high={pctContext > 85} title="Context used before older turns get condensed"><i style:width="{pctContext}%"></i></div>
          <span>{Math.round((info.last_prompt_tokens ?? 0) / 1000)}k / {Math.round((info.context_limit ?? 0) / 1000)}k</span>
          {#if info.condensed?.times}<span>· condensed {info.condensed.times}×</span>{/if}
          {#if info.cost}<span>· ≈ ${info.cost.toFixed(2)}</span>{/if}
          {#if cfg.engine === "antigravity"}
            <select class="small" style="width:auto;padding:2px 6px" bind:value={cfg.agy_model} onchange={() => save?.()} aria-label="Antigravity model" title="Switch who runs the GM in Antigravity">
              {#each AGY_MODELS as m}<option value={m.id}>{m.name}</option>{/each}
            </select>
          {:else if cfg.engine === "claude_code" || cfg.engine === "api"}
            <select class="small" style="width:auto;padding:2px 6px" bind:value={cfg.model} onchange={() => save?.()} aria-label="Claude model" title="Switch Claude model">
              {#each MODELS as m}<option value={m.id}>{m.name}</option>{/each}
            </select>
          {/if}
          <span class="grow" style="text-align:right">{memoryLine}</span>
          {#if entries.length}<button class="ghost small" onclick={endChat}>End chat…</button>{/if}
        </div>
      </div>
    </div>
  </div>

  <aside class="side" class:open={sideOpen}>
    <div class="side-tabs">
      <button class="tab narrow-only" style="flex:none" aria-label="Back to the story" onclick={() => (sideOpen = false)}>← Story</button>
      {#each [["status", "Status"], ["map", "🗺"], ["music", "🎵"], ["pics", "🖼"], ["voices", "💬"]] as [id, name]}
        <button class="tab" class:on={side === id} aria-label={id} onclick={() => { side = id; loadSide(); }}>{name}</button>
      {/each}
    </div>
    <div class="side-body">
      {#if side === "status"}
        <Status bind:rules only={hostPlays} />
      {:else if side === "pics"}
        {#if !pics.length}<p class="small muted">None yet. The GM draws maps, items and characters when they matter to the story.</p>{/if}
        {#each pics as p}<Art {p} />{/each}
      {:else if side === "map"}
        <div style="height:60vh;display:flex;flex-direction:column"><MapTab /></div>
        <Maps list={maps} />
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
      {/if}
    </div>
  </aside>
</div>

{#each info.activities ?? [] as a (a.id)}
  {#if !openActivity}
    <button class="toast" onclick={() => (openActivity = a)}>{a.icon ?? "✍"} {a.title ?? "An activity is open"}</button>
  {/if}
{/each}
<LedgerDock open={() => go("host:ledger")} />

{#if openActivity}
  <Activity a={openActivity}
    submit={async (text: string, id: number) => { openActivity = null; await call("activity_close", { id }); await send(text); }}
    close={async (id: number) => { openActivity = null; await call("activity_close", { id }); await refresh(); }} />
{/if}
