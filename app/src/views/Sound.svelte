<script lang="ts">
  import { call } from "../lib/api";
  import { music } from "../lib/music.svelte";

  // Sound is each person's own, host or player: it's saved on their computer.
  let { cfg } = $props();

  function apply() {
    music.setVolume(cfg.music_volume, cfg.music_muted);
    call("set_config", { cfg: $state.snapshot(cfg) });
  }
</script>

<div class="col">
  <label class="field"><span>Music volume: {Math.round(cfg.music_volume * 100)}%</span>
    <input type="range" min="0" max="1" step="0.05" bind:value={cfg.music_volume} oninput={apply} />
  </label>
  <label class="check"><input type="checkbox" bind:checked={cfg.music_muted} onchange={apply} /> <span>Mute the music</span></label>
  {#if music.title}
    <div class="row small"><span class="grow">🎵 {music.title}</span><button class="ghost small" onclick={() => music.stop()}>Stop</button></div>
  {/if}
</div>
