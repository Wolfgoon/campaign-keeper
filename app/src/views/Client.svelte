<script lang="ts">
  import { onMount, tick } from "svelte";
  import { call, onClient, md, toast } from "../lib/api";
  import { music } from "../lib/music.svelte";
  import { speakEntry, stopSpeaking } from "../lib/voice.svelte";
  import Entry from "./Entry.svelte";
  import Status from "./Status.svelte";
  import Activity from "./Activity.svelte";
  import Sound from "./Sound.svelte";
  import Voices from "./Voices.svelte";

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
  let characters = $state<string[]>([]);
  let activities = $state<any[]>([]);
  let closedActivities = $state<number[]>([]);
  let openActivity = $state<any>(null);
  let action = $state("");
  let newChar = $state("");
  let talk = $state("");
  let talkLog = $state<any[]>([]);
  let side = $state("party");
  let log: HTMLDivElement;
  let seenMusic = 0;
  let seenReplies = -1;
  let stick = true;

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
  }

  const me = $derived(t?.table?.seats?.find((s: any) => s.id === you));
  const offers = $derived(activities.filter((a) => !closedActivities.includes(a.id)));

  function send(msg: any) {
    call("client_send", { msg }).catch(toast);
  }

  function submit(pass = false) {
    if (!pass && !action.trim()) return;
    stopSpeaking();
    send({ t: "submit", action: action.trim(), pass });
    action = "";
  }

  function exit() {
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
    const withMusic = entries.filter((e) => e.music).length;
    if (withMusic > seenMusic) {
      const e = [...entries].reverse().find((x) => x.music);
      if (e.music.stop) music.stop();
      else if (e.music.score) music.play(e.music);
    }
    seenMusic = withMusic;
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
          activities = m.activities ?? [];
          talkLog = m.state.table?.talk ?? talkLog;
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
        case "goodbye": error = m.reason; phase = "join"; music.stop(); break;
      }
    });
    return () => void un.then((f) => f());
  });
</script>

{#if phase === "join"}
  <div class="view"><div class="welcome">
    <h1>Join a game</h1>
    <p class="muted">Play at a friend's table. They run the game and the AI; you need their address and join code, and no Claude account of your own.</p>
    <label class="field"><span>Your name</span><input bind:value={name} /></label>
    {#if found.length}
      <h3>Tables on your network</h3>
      {#each found as f}
        <button onclick={() => (addr = f.addr)}>{f.campaign} <span class="muted small">· {f.host}</span></button>
      {/each}
    {:else}
      <p class="small muted">{scanning ? "Looking… (tables the host chose to show appear here)" : "No tables found nearby."}</p>
    {/if}
    <label class="field"><span>Host's address</span><input bind:value={addr} placeholder="192.168.1.20 or a Tailscale name" /></label>
    <label class="field"><span>Join code</span><input class="mono" bind:value={code} placeholder="K7Q2-M9XD" /></label>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="row">
      <button class="primary" onclick={join}>Join</button>
      <button onclick={scan} disabled={scanning}>Look again</button>
      <button class="ghost" onclick={settings}>⚙ Settings</button>
      <button class="ghost" onclick={exit}>Back</button>
    </div>
    <p class="small muted">🔒 Connections are encrypted, and the join code is checked without ever crossing the network.</p>
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
        </div>
      </div>
      <div class="composer"><div class="composer-inner">
        {#each offers as a (a.id)}
          <button onclick={() => (openActivity = a)}>{a.icon ?? "✍"} {a.title ?? "An activity is open"}</button>
        {/each}
        <textarea rows="2" bind:value={action} placeholder={'What do you do?   > act   "speak"   /status'} aria-label="Your action" onkeydown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); submit(); } }}></textarea>
        <div class="row">
          <button class="primary" disabled={!me?.character} onclick={() => submit()}>Lock in</button>
          <button disabled={!me?.character} onclick={() => submit(true)}>Hold back this round</button>
          {#if !me?.character}<span class="small muted">Choose your character first.</span>
          {:else if me?.acted}<span class="small muted">✔ Your action is in. Waiting for the others.</span>{/if}
          <span class="grow"></span>
          <button class="ghost" aria-label="Settings" onclick={settings}>⚙</button>
          <button class="ghost" onclick={exit}>Leave the table</button>
        </div>
      </div></div>
    </div>
    <aside class="side">
      <div class="side-tabs">
        {#each [["party", "👥 Party"], ["status", "Status"], ["music", "🎵"], ["voices", "💬"]] as [id, label]}
          <button class="tab" class:on={side === id} aria-label={id} onclick={() => (side = id)}>{label}</button>
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
          {#if sheet}<Status {sheet} readOnly onSpend={(kind: string, name: string) => send({ t: "spend", kind, name })} />
          {:else}<p class="small muted">Your status window appears here once the host's game uses the rules and you've chosen a character.</p>{/if}
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
