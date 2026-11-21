<script lang="ts">
  import { onMount, tick } from "svelte";
  import { call, onClient, md, toast } from "../lib/api";
  import { music } from "../lib/music.svelte";
  import { platform } from "../lib/platform.svelte";
  import { speakEntry, stopSpeaking } from "../lib/voice.svelte";
  import Entry from "./Entry.svelte";
  import Status from "./Status.svelte";
  import Activity from "./Activity.svelte";
  import Sound from "./Sound.svelte";
  import Voices from "./Voices.svelte";
  import Maps from "./Maps.svelte";

  let { cfg, leave, settings } = $props();

  let phase = $state("join"); // join, waiting, seated
  let addr = $state(cfg?.join_addr ?? "");
  let code = $state("");
  let name = $state(cfg?.player_name ?? "");
  let found = $state<any[]>([]);
  let scanning = $state(false);
  let error = $state("");

  let entries = $state<any[]>([]);
  let stream = $state("");
  let t = $state<any>(null);
  let you = $state<number | null>(null);
  let busy = $state(false);
  let sheet = $state<any>(null);
  let labels = $state<any>({});
  let maps = $state<any[]>([]);
  let characters = $state<string[]>([]);
  let activities = $state<any[]>([]);
  let closedActivities = $state<number[]>([]);
  let openActivity = $state<any>(null);
  let action = $state("");
  // How the next action is taken: as a deed (the default) or as words.
  let saying = $state("do");
  // The party's shared page: everyone at the table reads and writes the same file.
  let noteText = $state("");
  let noteSaved = $state("");
  const noteChanged = $derived(noteText !== noteSaved);
  let newChar = $state("");
  let talk = $state("");
  let talkLog = $state<any[]>([]);
  let side = $state("party");
  let log: HTMLDivElement;
  let seenMusic = 0;
  let seenReplies = -1;
  let stick = true;
  let sideOpen = $state(false);
  let joining = $state(false);
  let reconnecting = $state(false);

  // An invitation from the host fills in both fields, so nobody types an address and a code.
  let pasted = $state("");
  let pasteNote = $state("");
  let pasteBad = $state(false);

  async function readInvite() {
    const text = pasted.trim();
    if (!text) {
      pasteNote = "";
      pasteBad = false;
      return;
    }
    try {
      const inv = (await call("pair_parse", { text })) as { addr: string; code: string };
      addr = inv.addr;
      code = inv.code;
      pasteNote = `Ready: ${inv.addr} · ${inv.code}`;
      pasteBad = false;
    } catch (e) {
      pasteNote = String(e);
      pasteBad = true;
    }
  }

  function nearBottom() {
    return !log || log.scrollHeight - log.scrollTop - log.clientHeight < 200;
  }

  async function scan() {
    scanning = true;
    try {
      found = await call("table_scan");
    } catch (e) {
      error = String(e);
    }
    scanning = false;
  }

  async function join() {
    if (joining) return;
    joining = true;
    error = "";
    try {
      await call("client_join", { addr, code, name });
      // The host's welcome can get here first, so only move on if it hasn't.
      if (phase === "join") phase = "waiting";
      cfg.join_addr = addr;
      cfg.player_name = name;
      await call("set_config", { cfg: $state.snapshot(cfg) });
    } catch (e) {
      error = String(e);
    }
    joining = false;
  }

  // The line dropped (a phone slept, Wi-Fi blinked): take the seat back without asking.
  async function reconnect() {
    if (reconnecting) return;
    reconnecting = true;
    for (let i = 0; i < 8 && reconnecting; i++) {
      await new Promise((r) => setTimeout(r, i ? 4000 : 800));
      if (!reconnecting) return;
      try {
        await call("client_join", { addr, code, name });
        reconnecting = false;
        return;
      } catch {
        /* the host may still be out of reach: try again */
      }
    }
    if (!reconnecting) return;
    reconnecting = false;
    error = "The connection to the host was lost.";
    phase = "join";
    music.stop();
  }

  const me = $derived(t?.table?.seats?.find((s: any) => s.id === you));
  const offers = $derived(activities.filter((a) => !closedActivities.includes(a.id)));
  // One-tap actions: they fill the box, so nothing is sent that the player hasn't seen.
  const QUICK = [["👀 Look", "I take a careful look around."], ["👂 Listen", "I listen for a moment."], ["⏳ Wait", "I wait and watch."]];

  function send(msg: any) {
    call("client_send", { msg }).catch(toast);
  }

  function submit(pass = false) {
    if (pass) {
      stopSpeaking();
      send({ t: "submit", pass });
      action = "";
      return;
    }
    const text = action.trim();
    if (!text) return;
    // Their own punctuation wins; otherwise the mode decides how it's heard.
    const body = /^[>/"~]/.test(text) ? text : saying === "say" ? `"${text}"` : `> ${text}`;
    stopSpeaking();
    send({ t: "submit", action: body });
    action = "";
  }

  function openSide(id: string) {
    side = id;
    sideOpen = true;
  }

  function saveNote() {
    noteSaved = noteText;
    send({ t: "note", text: noteText });
  }

  function exit() {
    reconnecting = false;
    phase = "join";
    call("client_leave");
    music.stop();
    stopSpeaking();
    leave();
  }

  function newStory() {
    const replies = entries.filter((e) => e.role === "assistant");
    if (seenReplies >= 0 && replies.length > seenReplies && cfg.tts_auto) {
      const last = replies[replies.length - 1];
      speakEntry(last.text, last.voices, cfg.tts_speed);
    }
    seenReplies = replies.length;
    const withSound = entries.filter((e) => e.music || e.ambience).length;
    if (withSound > seenMusic) {
      const last = [...entries].reverse();
      const m = last.find((x) => x.music);
      const a = last.find((x) => x.ambience);
      if (m) music.apply({ music: m.music });
      if (a) music.apply({ ambience: a.ambience });
    }
    seenMusic = withSound;
  }

  onMount(() => {
    scan();
    music.setVolume(cfg.music_volume ?? 0.5, cfg.music_muted ?? false);
    const un = onClient(async (m) => {
      switch (m.t) {
        case "waiting": phase = "waiting"; break;
        case "welcome": phase = "seated"; error = ""; break;
        case "table":
          t = m.state; you = m.you; busy = m.busy; sheet = m.sheet; characters = m.characters;
          labels = m.labels ?? {}; maps = m.maps ?? [];
          activities = m.activities ?? [];
          talkLog = m.state.table?.talk ?? talkLog;
          // Never pull the page out from under someone typing: their words win until they save.
          if (!noteChanged) {
            noteText = m.note ?? "";
            noteSaved = noteText;
          }
          break;
        case "transcript":
          // Follow the story only if they're at the bottom: never yank someone reading back.
          stick = nearBottom();
          entries = m.entries;
          stream = "";
          newStory();
          await tick();
          if (stick && log) log.scrollTop = log.scrollHeight;
          break;
        case "stream":
          stick = nearBottom();
          stream = m.text;
          await tick();
          if (stick && log) log.scrollTop = log.scrollHeight;
          break;
        case "talk": talkLog.push(m.line); break;
        case "notice": toast(m.text); break;
        case "refused":
        case "goodbye":
          if (m.lost && phase === "seated") reconnect();
          else if (!reconnecting) { error = m.reason; phase = "join"; music.stop(); }
          break;
      }
    });
    return () => void un.then((f) => f());
  });
</script>

{#if phase === "join"}
  <div class="view"><div class="welcome">
    <h1>Join a game</h1>
    <p class="muted">{platform.web ? "Play at your friend's table, right here in the browser. They run the game and the AI; you just need the join code they give you." : "Play at a friend's table. They run the game and the AI; you need their address and join code, and no Claude account of your own."}</p>
    <label class="field"><span>Your name</span><input bind:value={name} /></label>
    {#if !platform.web}
      <label class="field"><span>Invitation (paste what the host copied)</span>
        <input value={pasted} oninput={(e) => { pasted = e.currentTarget.value; readInvite(); }} placeholder="campaignkeeper://join?host=my-pc&code=K7Q2-M9XD" />
      </label>
      {#if pasteNote}<p class={pasteBad ? "small error" : "small muted"} role={pasteBad ? "alert" : "status"}>{pasteNote}</p>{/if}
    {/if}
    {#if platform.web}
      <!-- The page came from the host: there's no address to find or type. -->
    {:else if found.length}
      <h3>Tables on your network</h3>
      {#each found as f}
        <button onclick={() => (addr = f.addr)}>{f.campaign} <span class="muted small">· {f.host}</span></button>
      {/each}
    {:else}
      <p class="small muted">{scanning ? "Looking… (tables the host chose to show appear here)" : "No tables found nearby."}</p>
    {/if}
    {#if !platform.web}<label class="field"><span>Host's address</span><input bind:value={addr} placeholder="192.168.1.20 or a Tailscale name" /></label>{/if}
    <label class="field"><span>Join code</span><input class="mono" bind:value={code} placeholder="K7Q2-M9XD" /></label>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="row">
      <button class="primary" disabled={joining} onclick={join}>{joining ? "Joining…" : "Join"}</button>
      {#if !platform.web}<button onclick={scan} disabled={scanning}>Look again</button>{/if}
      <button class="ghost" onclick={settings}>⚙ Settings</button>
      {#if !platform.web}<button class="ghost" onclick={exit}>Back</button>{/if}
    </div>
    {#if platform.web}
      <p class="small muted">Tip: add this page to your home screen to open it like an app.</p>
    {:else}
      <p class="small muted">🔒 Connections are encrypted, and the join code is checked without ever crossing the network.</p>
    {/if}
  </div></div>
{:else if phase === "waiting"}
  <div class="view"><div class="welcome"><h2>Waiting for the host to let you in…</h2><button onclick={exit}>Leave</button></div></div>
{:else}
  <div class="play">
    <div class="story">
      <div class="log" bind:this={log}>
        <div class="log-inner">
          {#if !entries.length}<p class="muted">The story will appear here once the host begins.</p>{/if}
          {#each entries as e, i (i)}<Entry {e} speak={(x: any) => speakEntry(x.text, x.voices, cfg.tts_speed)} />{/each}
          {#if stream}<div class="msg-gm">{@html md(stream)}</div>{/if}
          {#if busy && !stream}<div class="msg-note">The GM is writing…</div>{/if}
          {#if reconnecting}<div class="msg-note" role="status">Connection lost. Reconnecting to the host…</div>{/if}
        </div>
      </div>
      <div class="composer"><div class="composer-inner">
        {#each offers as a (a.id)}
          <button onclick={() => (openActivity = a)}>{a.icon ?? "✍"} {a.title ?? "An activity is open"}</button>
        {/each}
        <div class="row modes" role="group" aria-label="Actions">
          <button class:on={saying === "do"} aria-pressed={saying === "do"} title="Your next words are something your character does" onclick={() => (saying = "do")}>&gt; Do</button>
          <button class:on={saying === "say"} aria-pressed={saying === "say"} title="Your next words are something your character says" onclick={() => (saying = "say")}>" Say</button>
          {#each QUICK as [label, words]}
            <button class="ghost small" onclick={() => (action = words)}>{label}</button>
          {/each}
          <span class="grow"></span>
          <button class="ghost small" onclick={() => openSide("party")}>👥 Party</button>
          <button class="ghost small" onclick={() => openSide("status")}>📊 Sheet</button>
          <button class="ghost small" onclick={() => openSide("note")}>📜 Notes</button>
        </div>
        <textarea rows="2" bind:value={action} placeholder={'What do you do?   > act   "speak"   /status'} aria-label="Your action" onkeydown={(e) => { if (e.key === "Enter" && !e.shiftKey && !platform.mobile) { e.preventDefault(); submit(); } }}></textarea>
        <div class="row">
          <button class="primary" disabled={!me?.character} onclick={() => submit()}>Lock in</button>
          <button disabled={!me?.character} onclick={() => submit(true)}>Hold back this round</button>
          {#if !me?.character}<span class="small muted">Choose your character first.</span>
          {:else if me?.acted}<span class="small muted">✔ Your action is in. Waiting for the others.</span>{/if}
          <span class="grow"></span>
          <button class="narrow-only" onclick={() => (sideOpen = true)}>☰ Party, status, map…</button>
          <button class="ghost" aria-label="Settings" onclick={settings}>⚙</button>
          <button class="ghost" onclick={exit}>Leave the table</button>
        </div>
      </div></div>
    </div>
    <aside class="side" class:open={sideOpen || !me?.character}>
      <div class="side-tabs">
        {#if me?.character}<button class="tab narrow-only" style="flex:none" aria-label="Back to the story" onclick={() => (sideOpen = false)}>← Story</button>{/if}
        {#each [["party", "👥"], ["status", "Status"], ["note", "📜"], ["map", "🗺"], ["music", "🎵"], ["voices", "💬"]] as [id, label]}
          <button class="tab" class:on={side === id} aria-label={id === "note" ? "Party notes" : id === "party" ? "Party" : id} onclick={() => (side = id)}>{label}</button>
        {/each}
      </div>
      <div class="side-body col">
        {#if side === "party"}
          {#if !me?.character}
            <h3>Choose your character</h3>
            {#each characters as c}<button onclick={() => send({ t: "claim", character: c })}>{c}</button>{/each}
            {#if !characters.length}<p class="small muted">This campaign has no characters yet: make one below.</p>{/if}
            <input bind:value={newChar} placeholder="Or a new character's name" aria-label="New character's name" />
            <button disabled={!newChar.trim()} onclick={() => send({ t: "claim", character: newChar, new: true })}>Make & take</button>
          {:else}
            <p class="small">You play <b>{me.character}</b>.</p>
          {/if}
          <h3>The party</h3>
          <div class="small">★ {t?.host}{t?.host_is_gm ? " (Game Master)" : t?.host_character ? ` as ${t.host_character}` : ""} {t?.table?.host_action || t?.table?.narration ? "✔" : ""}</div>
          {#each t?.table?.seats ?? [] as s}
            {#if s.admitted}<div class="small">{s.connected ? "●" : "○"} {s.name}{s.character ? ` as ${s.character}` : ""} {s.acted ? "✔" : ""}</div>{/if}
          {/each}
          <h3>Table talk</h3>
          <p class="small muted">The AI never sees it.</p>
          {#each talkLog as l}<div class="small"><b>{l.from}:</b> {l.text}</div>{/each}
          <input bind:value={talk} placeholder="Say something to the table" aria-label="Say something to the table" onkeydown={(e) => { if (e.key === "Enter") { send({ t: "talk", text: talk }); talk = ""; } }} />
        {:else if side === "status"}
          {#if sheet}<Status {sheet} {labels} readOnly onSpend={(kind: string, name: string) => send({ t: "spend", kind, name })} />
          {:else}<p class="small muted">Your status window appears here once the host's game uses the rules and you've chosen a character.</p>{/if}
        {:else if side === "note"}
          <h3>Party notes</h3>
          <p class="small muted">Everyone at the table can read and write this page. The GM sees it with the campaign's files.</p>
          <textarea rows="10" bind:value={noteText} placeholder="What the party has found, promised, or plans to do…" aria-label="Party notes"></textarea>
          <div class="row">
            <button class="primary" disabled={!noteChanged} onclick={saveNote}>Save the notes</button>
            <span class="small muted">{noteChanged ? "Unsaved changes" : "Saved"}</span>
          </div>
        {:else if side === "map"}
          <Maps list={maps} />
        {:else if side === "music"}
          <Sound {cfg} />
        {:else}
          <Voices {cfg} />
        {/if}
      </div>
    </aside>
  </div>
{/if}

{#if openActivity}
  <Activity a={openActivity}
    submit={(text: string, id: number) => { openActivity = null; closedActivities.push(id); send({ t: "submit", action: text }); }}
    close={(id: number) => { openActivity = null; closedActivities.push(id); }} />
{/if}
