<script lang="ts">
  import { safeSvg } from "../lib/api";

  // One of the GM's pictures: pixel art when there is some, otherwise the plain drawing.
  let { p } = $props();

  // Only images the app made itself reach the page this way.
  let scale = $state(0);
  const png = $derived(typeof p.png === "string" && p.png.startsWith("data:image/png;base64,") ? p.png : null);
</script>

<figure class="pic" style="margin-inline:0">
  {#if png}
    <!-- Whole-number scaling keeps every pixel square, and a small sprite stays small. -->
    <img src={png} alt={p.title ?? "A picture from the GM"} style:max-width="{scale ? `${scale}px` : "100%"}" onload={(e) => { const w = (e.currentTarget as HTMLImageElement).naturalWidth; scale = w * Math.max(1, Math.min(8, Math.floor(460 / w))); }} />
  {:else}
    {@html safeSvg(p.svg)}
  {/if}
  {#if p.title}<figcaption>{p.title}</figcaption>{/if}
</figure>
