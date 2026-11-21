<script lang="ts">
  import { onMount } from "svelte";
  import { call, onTable, toast } from "../lib/api";
  import Status from "./Status.svelte";

  // The table's shared page. Players get the same notes and the same sheet in their own panel,
  // so the host is reading what they read.
  let { go } = $props();
  let table = $state<any>(null);
  let rules = $state<any>(null);
  let note = $state("");
  let saved = $state("");
  let saving = $state(false);
  const changed = $derived(note !== saved);
  const seats = $derived((table?.table?.seats ?? []).filter((s: any) => s.admitted));
  // A host playing a character is a player: their sheet, never everyone else's.
  const mine = $derived<string | null>(table && !table.host_is_gm ? (table.host_character ?? "") : null);

  async function load() {
    table = await call("table_state");
    rules = await call("rpg_state");
    const text = await call<string>("party_notes");
    // Someone may have typed here before the page arrived; their words win.
    if (note === saved) {
      note = text;
    }
    saved = text;
  }

  async function save() {
    saving = true;
    // Our own change is not "someone else edited this": keep the notice for real outside edits.
    saved = note;
    try {
      await call("party_note", { text: note });
      toast("Saved the party's notes.");
    } catch (e) {
      toast(e);
      // Still unsaved: compare against what is actually on disk.
      try {
        saved = await call<string>("party_notes");
      } catch {
        saved = "";
      }
    }
    saving = false;
  }

  onMount(() => {
    load();
    // A player may write this page while the host is reading it.
    const un = onTable(async () => {
      table = await call("table_state");
      const text = await call<string>("party_notes");
      if (note === saved) {
        note = text;
        saved = text;
      } else if (text !== saved) {
        saved = text;
        toast("The party's notes changed. Your unsaved words are still in the box.");
      }
    });
    return () => {
      un.then((f) => f());
    };
  });
</script>

<div class="pad scroll grow" style="max-width:900px;margin:0 auto">
  <h1>Party</h1>
  <p class="muted">What everyone at the table shares: the party's notes, and your character's sheet.</p>

  {#if table}
    <div class="card">
      <div class="row small">
        <span class="grow">At the table: <b>{table.host}</b>{table.host_is_gm ? " (Game Master)" : table.host_character ? ` as ${table.host_character}` : ""}</span>
        <button class="small" onclick={() => go("host")}>🛠 Host tools</button>
      </div>
      {#each seats as s}
        <div class="small muted">{s.connected ? "●" : "○"} {s.name}{s.character ? ` as ${s.character}` : ""}{s.acted ? " ✔" : ""}</div>
      {/each}
    </div>
  {:else}
    <p class="small muted">Notes work while playing alone too. Open the table under 🛠 Host when friends join.</p>
  {/if}

  <h2 style="margin-top:18px">Party notes</h2>
  <textarea rows="9" bind:value={note} placeholder="What the party has found, promised, or plans to do…" aria-label="Party notes"></textarea>
  <div class="row">
    <button class="primary" disabled={!changed || saving} onclick={save}>{saving ? "Saving…" : "Save the notes"}</button>
    <span class="small muted">{changed ? "Unsaved changes" : "Saved"}</span>
  </div>

  {#if rules?.enabled}
    <h2 style="margin-top:18px">{mine === null ? "Sheets" : mine ? `Your sheet: ${mine}` : "Your sheet"}</h2>
    <Status {rules} only={mine} />
  {/if}
</div>
