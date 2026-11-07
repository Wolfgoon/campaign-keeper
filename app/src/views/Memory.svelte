<script lang="ts">
  import { onMount } from "svelte";
  import { call, md, toast } from "../lib/api";

  let status = $state<any>(null);
  let list = $state<any[]>([]);
  let q = $state("");
  let hits = $state<any[] | null>(null);
  let adding = $state("");
  let working = $state(false);
  let tab = $state("memories");

  const embedLabel = $derived.by(() => {
    const e = status?.embeddings ?? "off";
    if (e === "ready") return status.pending ? `Search by meaning: indexing, ${status.pending} left` : "Search by meaning: ready";
    if (e === "loading") return "Search by meaning: getting the model ready (about 130 MB, once). Word search works now.";
    if (e.startsWith("failed")) return "Search by meaning isn't available, so recall uses word search.";
    return "Search by meaning: off";
  });

  async function load() {
    status = await call("memory_status");
    list = await call("memory_list");
  }

  async function search() {
    hits = q.trim() ? await call("memory_search", { query: q }) : null;
  }

  async function add() {
    await call("memory_add", { text: adding, kind: "fact", importance: 0.7 });
    adding = "";
    await load();
  }

  async function catchUp() {
    working = true;
    try {
      toast(await call("memory_catch_up"));
      await load();
    } catch (e) {
      toast(e);
    }
    working = false;
  }

  const shown = $derived(hits ?? list);
  const stars = (i: number) => (i >= 0.8 ? "★★★" : i >= 0.5 ? "★★" : "★");

  onMount(load);
</script>

<div class="pad scroll grow" style="max-width:940px;margin:0 auto">
  <h1>Memory</h1>
  <p class="muted">What the campaign remembers. The GM only ever gets the parts that matter for the moment, so long games stay sharp and cheap. Nothing here can spoil the story: the Keeper only reads what you've already seen.</p>
  {#if status}
    <div class="row" style="gap:22px;margin-bottom:12px">
      <div><div class="stat">{(status.counts.fact ?? 0) + (status.counts.event ?? 0) + (status.counts.npc ?? 0) + (status.counts.place ?? 0) + (status.counts.promise ?? 0)}</div><div class="small muted">memories</div></div>
      <div><div class="stat">{status.counts.turn ?? 0}</div><div class="small muted">exchanges word for word</div></div>
      <div><div class="stat">{status.counts.file ?? 0}</div><div class="small muted">file passages</div></div>
      <div><div class="stat">{status.condensed?.times ?? 0}</div><div class="small muted">times condensed</div></div>
      <button class="grow" style="max-width:220px" disabled={working} onclick={catchUp} title="Run the Keeper over everything played so far">{working ? "Filing…" : "File this game so far"}</button>
    </div>
    <p class="small muted">{embedLabel}</p>
  {/if}
  <div class="row" style="margin-bottom:8px;border-bottom:1px solid var(--line)">
    {#each [["memories", "Memories"], ["story", "Story so far"], ["journal", "Journal"], ["style", "How you play"]] as [id, name]}
      <button class="tab" class:on={tab === id} onclick={() => (tab = id)}>{name}</button>
    {/each}
  </div>

  {#if tab === "memories"}
    <input bind:value={q} oninput={search} placeholder="Ask by meaning: who owes me money?" aria-label="Search memories" />
    <div class="row" style="margin:8px 0">
      <input class="grow" style="width:auto" bind:value={adding} placeholder="Add something the GM must remember" aria-label="Add a memory" />
      <button disabled={!adding.trim()} onclick={add}>+ Add</button>
    </div>
    {#if !shown.length}<p class="muted">Nothing yet. The Keeper files memories every few replies.</p>{/if}
    {#each shown as m (m.id)}
      <div class="mem">
        <div class="kind">{m.kind}<br /><span title="Importance">{stars(m.importance)}</span></div>
        <div class="grow" style="white-space:pre-wrap;overflow-wrap:anywhere">{m.text}</div>
        {#if !["turn", "file"].includes(m.kind)}
          <button class="ghost danger small" aria-label="Forget this" onclick={async () => { await call("memory_delete", { id: m.id }); await load(); await search(); }}>✖</button>
        {/if}
      </div>
    {/each}
  {:else if tab === "story"}
    {#if status?.condensed?.text}<div class="card">{@html md(status.condensed.text)}</div>
    {:else}<p class="muted">The chat hasn't been condensed yet. That happens once it passes the context limit in Settings. Nothing is ever deleted: older turns just stop being sent, and this list goes in their place.</p>{/if}
  {:else if tab === "journal"}
    {#if status?.journal}<div>{@html md(status.journal)}</div>{:else}<p class="muted">The Keeper writes a short entry for each stretch of play.</p>{/if}
  {:else}
    <p class="muted small">Learned across all your campaigns. Patterns grow stronger when seen again and fade when contradicted. Strong ones go to the GM.</p>
    {#if !status?.playstyle?.length}<p class="muted">Nothing noticed yet.</p>{/if}
    {#each status?.playstyle ?? [] as h}
      <div class="mem"><div class="kind">{h.strength.toFixed(2)}</div><div class="grow">{h.text}</div></div>
    {/each}
  {/if}
</div>
