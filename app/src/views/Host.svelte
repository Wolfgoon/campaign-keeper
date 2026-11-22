<script lang="ts">
  import { onMount } from "svelte";
  import { call, onTable } from "../lib/api";
  import Table from "./Table.svelte";
  import Files from "./Files.svelte";
  import Scribe from "./Scribe.svelte";
  import Memory from "./Memory.svelte";
  import Search from "./Search.svelte";

  // Everything only the host may see lives behind this one tab. Every other tab is what the whole
  // table sees, so the host never works in a different app from their players.
  let { cfg, part = $bindable("table") } = $props();
  let table = $state<any>(null);
  let loaded = $state(false);

  const PARTS = [
    ["table", "👥 Table"],
    ["files", "📁 Files"],
    ["scribe", "✏ Scribe"],
    ["memory", "🧠 Memory"],
    ["search", "🔍 Search"],
  ];

  onMount(() => {
    call("table_state").then((t: any) => (table = t)).catch(() => {}).finally(() => (loaded = true));
    const un = onTable(async () => {
      try {
        table = await call("table_state");
      } catch {
        /* the campaign closed */
      }
    });
    return () => {
      un.then((f) => f());
    };
  });
</script>

<div class="col grow" style="min-height:0;gap:0">
  <div class="side-tabs" role="tablist" aria-label="Host tools">
    {#each PARTS as [id, name]}
      <button class="tab" class:on={part === id} onclick={() => (part = id)}>{name}</button>
    {/each}
  </div>
  <div class="view" style="min-height:0">
    {#if part === "table"}
      <div class="pad scroll grow" style="max-width:820px;margin:0 auto">
        {#if loaded}<Table bind:table {cfg} />{:else}<p class="small muted">Looking for a table…</p>{/if}
      </div>
    {:else if part === "files"}
      <Files />
    {:else if part === "scribe"}
      <Scribe {cfg} />
    {:else if part === "memory"}
      <Memory />
    {:else}
      <Search />
    {/if}
  </div>
</div>
