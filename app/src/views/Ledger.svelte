<script lang="ts">
  import { onMount } from "svelte";
  import { call, md, toast } from "../lib/api";

  // The Ledger: a chat just for the status windows. It proposes changes; you apply them.
  let history = $state<any[]>([]);
  let input = $state("");
  let working = $state(false);
  // Per answer, which proposed lines are still ticked.
  let picked = $state<Record<number, Record<string, boolean>>>({});
  let list = $state<HTMLDivElement>();

  const ASKS = [
    ["📒 Do the books", "Do the books: go through the recent story and catch my status up on everything it doesn't show yet: items gained, used, sold, given or lost, money spent or earned, injuries, rest and conditions."],
    ["🎒 Check my inventory", "Check my inventory against the recent story. Is anything there that I no longer have, or missing that I picked up? Propose fixes."],
    ["💰 Money", "Go over every payment and purchase in the recent story and make sure my money is right."],
    ["❤ Health", "Make sure my health and conditions match the recent story (injuries, rest, healing, poison)."],
  ];

  function tick(h: any[]) {
    history = h;
    h.forEach((l, i) => {
      if (l.changes?.length && !picked[i]) picked[i] = Object.fromEntries(l.changes.map((c: string) => [c, true]));
    });
    setTimeout(() => list?.scrollTo({ top: list.scrollHeight, behavior: "smooth" }), 30);
  }

  onMount(() => {
    call("ledger_history").then(tick).catch(() => {});
  });

  async function ask(text = input) {
    if (!text.trim() || working) return;
    working = true;
    input = "";
    history = [...history, { role: "user", text }];
    try {
      tick(await call("ledger_ask", { message: text }));
    } catch (e) {
      toast(e);
      input = text;
      history = history.slice(0, -1);
    }
    working = false;
  }

  async function apply(i: number) {
    const chosen = Object.entries(picked[i] ?? {}).filter(([, v]) => v).map(([c]) => c);
    try {
      tick(await call("ledger_apply", { index: i, picked: chosen }));
      toast("Status updated. The GM will see what changed on its next turn.");
    } catch (e) {
      toast(e);
    }
  }

  async function discard(i: number) {
    tick(await call("ledger_discard", { index: i }));
  }

  async function clear() {
    if (!confirm("Clear the Ledger's conversation? Applied changes stay applied.")) return;
    await call("ledger_clear");
    picked = {};
    history = [];
  }
</script>

<div class="col grow pad" style="min-height:0;max-width:820px;margin:0 auto;width:100%">
  <div class="row">
    <h2 class="grow" style="margin:0">📒 Ledger</h2>
    {#if history.length}<button class="ghost small" onclick={clear}>Clear</button>{/if}
  </div>
  <p class="small muted" style="margin:0">Keep the status windows straight, outside the story. Tell it what's wrong in plain words ("I sold the lamp for 4 silver", "add the rope from last session"). It proposes the changes; nothing happens until you press Apply. The GM is told what changed.</p>

  <div class="col grow scroll" style="min-height:0;gap:10px" bind:this={list}>
    {#if !history.length}
      <p class="muted">Nothing yet. Try one of these:</p>
    {/if}
    {#each history as l, i}
      {#if l.role === "user"}
        <div class="card" style="align-self:flex-end;max-width:85%;background:var(--panel-2)">{l.text}</div>
      {:else}
        <div class="card col" style="max-width:92%">
          {#if l.auto}<div class="small muted">🕑 Automatic check</div>{/if}
          {#if l.text}<div>{@html md(l.text)}</div>{/if}
          {#if l.changes?.length}
            <div class="col" style="gap:4px">
              {#if l.results?.length}
                {#each l.results as r}<div class="small mono" style:color={r.startsWith("Refused") ? "var(--danger, #e55)" : "var(--gold)"}>{r.startsWith("Refused") ? "✗" : "✓"} {r}</div>{/each}
              {:else if l.discarded}
                {#each l.changes as c}<div class="small mono muted" style="text-decoration:line-through">{c}</div>{/each}
              {:else}
                {#each l.changes as c}
                  <label class="check small mono"><input type="checkbox" bind:checked={picked[i][c]} /> <span>{c}</span></label>
                {/each}
                <div class="row">
                  <button class="primary" onclick={() => apply(i)}>Apply</button>
                  <button class="ghost" onclick={() => discard(i)}>Discard</button>
                </div>
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    {/each}
    {#if working}<p class="small muted">The Ledger is checking the books…</p>{/if}
  </div>

  <div class="row" style="flex-wrap:wrap">
    {#each ASKS as [name, text]}<button class="small" disabled={working} onclick={() => ask(text)}>{name}</button>{/each}
  </div>
  <div class="send-row">
    <textarea rows="2" bind:value={input} placeholder="What needs fixing? e.g. I gave Mira two healing potions" aria-label="Message to the Ledger"
      onkeydown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); ask(); } }}></textarea>
    <button class="primary" disabled={working || !input.trim()} onclick={() => ask()}>Send</button>
  </div>
</div>
