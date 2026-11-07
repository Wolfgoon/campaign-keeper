<script lang="ts">
  import { call } from "../lib/api";

  let q = $state("");
  let hits = $state<any[]>([]);
  let timer: any;

  function run() {
    clearTimeout(timer);
    timer = setTimeout(async () => (hits = await call("search_files", { query: q })), 180);
  }
</script>

<div class="pad scroll grow" style="max-width:900px;margin:0 auto">
  <h1>Search</h1>
  <input bind:value={q} oninput={run} placeholder="Search every campaign file…" aria-label="Search every campaign file" />
  <p class="small muted" style="margin-top:6px">{q.length < 2 ? "Type at least two letters." : `${hits.length} matches. To search by meaning, use the Memory tab.`}</p>
  {#each hits as h}
    <div class="mem">
      <div class="grow"><div class="small" style="color:var(--gold)">{h.path}:{h.line}</div>{h.text}</div>
    </div>
  {/each}
</div>
