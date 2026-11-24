<script lang="ts">
  import { onMount } from "svelte";
  import { call, onGame, toast } from "../lib/api";

  // A small window in the bottom-left corner: the Ledger's proposed status changes, waiting to be applied.
  let { open }: { open: () => void } = $props();
  let history = $state<any[]>([]);
  let small = $state(false);

  const pending = $derived(history.map((l, i) => ({ ...l, i })).filter((l) => l.changes?.length && !l.results?.length && !l.discarded));

  const load = async () => (history = await call<any[]>("ledger_history").catch(() => []));

  onMount(() => {
    load();
    const un = onGame((kind) => {
      if (kind === "ledger" || kind === "entry") load();
    });
    return () => void un.then((f) => f());
  });

  async function apply(l: any) {
    try {
      history = await call("ledger_apply", { index: l.i, picked: l.changes });
      toast("Status updated.");
    } catch (e) {
      toast(e);
    }
  }
  async function dismiss(l: any) {
    history = await call("ledger_discard", { index: l.i });
  }
</script>

{#if pending.length}
  <div class="ledger-dock card col" role="status" aria-label="The Ledger's proposed changes">
    <div class="row">
      <b class="grow">📒 Ledger · {pending.reduce((n, l) => n + l.changes.length, 0)} change{pending.reduce((n, l) => n + l.changes.length, 0) === 1 ? "" : "s"}</b>
      <button class="ghost small" onclick={() => (small = !small)} aria-label={small ? "Show" : "Hide"}>{small ? "▴" : "▾"}</button>
    </div>
    {#if !small}
      <div class="col scroll" style="gap:8px;max-height:40vh">
        {#each pending as l (l.i)}
          <div class="col" style="gap:3px">
            {#if l.text}<div class="small muted">{l.text}</div>{/if}
            {#each l.changes as c}<div class="small mono">• {c}</div>{/each}
            <div class="row">
              <button class="primary small" onclick={() => apply(l)}>Apply</button>
              <button class="ghost small" onclick={() => dismiss(l)}>Dismiss</button>
            </div>
          </div>
        {/each}
      </div>
      <button class="ghost small" onclick={open}>Open the Ledger…</button>
    {/if}
  </div>
{/if}

<style>
  .ledger-dock { position: fixed; left: 12px; bottom: 12px; z-index: 15; width: min(340px, calc(100vw - 24px)); gap: 6px; box-shadow: 0 6px 24px rgb(0 0 0 / 35%); }
</style>
