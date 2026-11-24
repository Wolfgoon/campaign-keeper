<script lang="ts">
  import { onMount } from "svelte";
  import { call, onGame } from "../lib/api";
  import TileMap from "./TileMap.svelte";

  // The host's Map tab: loads the picture map and reloads it whenever the helper redraws it.
  let data = $state<any>(null);
  const load = async () => (data = await call("tilemap").catch(() => null));

  onMount(() => {
    load();
    // Draw any art still missing; it arrives piece by piece.
    call("tileart_fill").catch(() => {});
    const un = onGame((kind) => {
      if (kind === "tilemap") load();
    });
    return () => void un.then((f) => f());
  });
</script>

<div class="view" style="display:flex;flex-direction:column;min-height:0"><TileMap {data} redraw={() => { if (confirm("Throw away this world's map art and draw it all again?")) call("tileart_fill", { redo: true }).then(load); }} /></div>
