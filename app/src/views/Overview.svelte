<script lang="ts">
  import { onMount } from "svelte";
  import { call, md } from "../lib/api";

  let { go } = $props();
  let o = $state<any>(null);
  let rules = $state<any>(null);

  onMount(async () => {
    o = await call("overview");
    rules = await call("rpg_state");
  });
</script>

<div class="pad scroll grow">
  {#if o}
    <h1>{o.name}</h1>
    <p class="muted">{o.setting ?? "No setting yet."}{o.difficulty ? ` · ${o.difficulty}` : ""}</p>
    <div class="row" style="margin-bottom:14px">
      <button class="primary" onclick={() => go("play")}>▶ Play</button>
      <button onclick={() => go("scribe")}>✏ Update files with the Scribe</button>
    </div>
    <div class="row" style="gap:22px;margin-bottom:16px">
      {#each Object.entries(o.counts) as [k, v]}
        <div><div class="stat">{v}</div><div class="small muted">{k}</div></div>
      {/each}
      <div><div class="stat">{Math.round(o.file_tokens / 1000)}k</div><div class="small muted">tokens of files</div></div>
    </div>
    <div class="row" style="align-items:flex-start;gap:14px">
      <div class="card grow" style="min-width:280px">
        <h2>Quests</h2>
        {#if o.quests}<div>{@html md(o.quests)}</div>{:else}<p class="muted">No quests yet.</p>{/if}
      </div>
      <div class="card grow" style="min-width:280px">
        <h2>Pinned memory</h2>
        {#if o.pinned}<div>{@html md(o.pinned)}</div>{:else}<p class="muted">Nothing pinned by the player yet. Use /remember while playing.</p>{/if}
      </div>
    </div>
    {#if rules?.enabled}
      <div class="card" style="margin-top:14px">
        <h2>Party</h2>
        {#each Object.values(rules.sheets) as s}
          {@const c = s as any}
          <p class="mono small">{c.name}: level {c.level} {c.class} · HP {c.hp.cur}/{c.hp.max}</p>
        {/each}
      </div>
    {/if}
    <div class="card" style="margin-top:14px">
      <h2>After each session</h2>
      <ol style="margin:0;padding-left:20px">
        <li>Type /save and store the SAVE BLOCK.{#if o.latest_save}<span class="muted small"> Latest: {o.latest_save}</span>{/if}</li>
        <li>Run the Scribe to update the character, NPCs, quests, map, and session log.</li>
        <li>Review each proposed change before it's written.</li>
      </ol>
    </div>
  {/if}
</div>
