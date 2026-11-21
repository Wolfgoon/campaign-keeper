<script lang="ts">
  // The GM's maps, read-only: it keeps them, everyone can look.
  let { list = [] } = $props();
  let chosen = $state("");
  let big = $state(false);

  const m = $derived(list.find((x: any) => x.name === chosen) ?? list[list.length - 1]);
</script>

{#if !list.length}
  <p class="small muted">No maps yet. The GM draws them as you explore, and keeps them up to date.</p>
{:else}
  {#if list.length > 1}
    <select value={m?.name} onchange={(e) => (chosen = e.currentTarget.value)} aria-label="Which map">
      {#each list as x}<option value={x.name}>{x.name}</option>{/each}
    </select>
  {/if}
  <div class="row"><b class="grow">{m?.name}</b><button class="ghost small" onclick={() => (big = true)}>⛶ Open large</button></div>
  <pre class="map-pre" aria-label="Map of {m?.name}">{m?.text}</pre>
  {#if m?.key}<pre class="small mono" style="margin:0;white-space:pre-wrap">{m.key}</pre>{/if}
{/if}

{#if big && m}
  <div class="map-wide" role="dialog" aria-label={m.name}>
    <div class="row"><h2 class="grow" style="margin:0">{m.name}</h2><button onclick={() => (big = false)}>Close</button></div>
    <pre class="map-pre">{m.text}</pre>
    {#if m.key}<pre class="small mono" style="margin:0;white-space:pre-wrap">{m.key}</pre>{/if}
  </div>
{/if}
