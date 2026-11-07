<script lang="ts">
  import { call } from "../lib/api";
  import { voices, cast, setCast, stopSpeaking } from "../lib/voice.svelte";

  let { cfg } = $props();

  const save = () => call("set_config", { cfg: $state.snapshot(cfg) });
</script>

<div class="col">
  <label class="check"><input type="checkbox" bind:checked={cfg.tts_auto} onchange={() => { if (!cfg.tts_auto) stopSpeaking(); save(); }} /> <span>Read the GM's replies aloud as they arrive</span></label>
  <label class="field"><span>Reading speed: {cfg.tts_speed.toFixed(2)}x</span><input type="range" min="0.6" max="1.6" step="0.05" bind:value={cfg.tts_speed} onchange={save} /></label>
  <button onclick={stopSpeaking}>⏹ Stop reading</button>
  {#if Object.keys(cast).length}
    <p class="small muted">Each character gets a voice the first time they speak, and keeps it.</p>
  {/if}
  {#each Object.keys(cast) as name}
    <label class="field"><span>{name}</span>
      <select value={cast[name]} onchange={(e) => setCast(name, e.currentTarget.value)}>
        {#each voices() as v}<option value={v.name}>{v.name}</option>{/each}
      </select>
    </label>
  {/each}
</div>
