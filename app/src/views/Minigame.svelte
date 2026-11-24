<script lang="ts">
  import { onMount } from "svelte";

  // Hands-on minigames the GM can open. Each ends by handing back one plain sentence saying what
  // the player did and how well; the GM narrates the outcome from it. Settings come from the
  // <activity> tag's attributes and its `- word: value` lists.
  let { a, done }: { a: any; done: (text: string) => void } = $props();

  const L = (k: string): string[] => a.lists?.[k] ?? [];
  const num = (v: any, d: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, Math.round(Number(v)) || d));
  const difficulty = num(a.difficulty, 2, 1, 5);
  const title = a.title ?? "it";

  // A shuffle that's the same each time the tab opens, so reopening it doesn't reroll the puzzle.
  function seeded(seed: string) {
    let h = 2166136261;
    for (const c of seed) h = Math.imul(h ^ c.charCodeAt(0), 16777619);
    return () => ((h = Math.imul(h ^ (h >>> 15), 2246822507) >>> 0), (h >>> 0) / 4294967296);
  }
  function shuffle<T>(xs: T[], r: () => number) {
    const v = [...xs];
    for (let i = v.length - 1; i > 0; i--) { const j = Math.floor(r() * (i + 1)); [v[i], v[j]] = [v[j], v[i]]; }
    return v;
  }

  // A loop for the moving games, stopped when the tab closes.
  let t = $state(0);
  onMount(() => {
    let raf = 0;
    const start = performance.now();
    const tick = (now: number) => { t = (now - start) / 1000; raf = requestAnimationFrame(tick); };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  });

  // --- lockpick: stop the moving pick inside the sweet spot, once per pin -------------------
  const pins = num(a.pins, 3, 1, 8);
  let picks = $state(num(a.picks, 3, 1, 9));
  let set = $state(0);
  let lockMsg = $state("");
  const zone = 0.22 - difficulty * 0.03;
  const zoneAt = $derived(0.2 + ((set * 0.37) % 0.6));
  const pickPos = $derived((Math.sin(t * (1.6 + difficulty * 0.45 + set * 0.2)) + 1) / 2);
  function tryPin() {
    if (set >= pins || picks <= 0) return;
    if (Math.abs(pickPos - (zoneAt + zone / 2)) <= zone / 2) { set++; lockMsg = set >= pins ? "Click! It's open." : "A pin sets."; }
    else { picks--; lockMsg = picks ? "Snap: a pick breaks." : "Your last pick breaks."; }
  }
  const lockOver = $derived(set >= pins || picks <= 0);

  // --- dice: real rolls, shown tumbling ----------------------------------------------------
  const diceSpec = (a.dice ?? "1d20") as string;
  const dm = /^(\d{0,2})d(\d{1,3})([+-]\d{1,3})?$/i.exec(diceSpec.replace(/\s/g, "")) ?? ["", "1", "20", ""];
  const dN = num(dm[1] || 1, 1, 1, 12), dS = num(dm[2], 20, 2, 100), dK = Number(dm[3] || 0);
  let rolls = $state<number[]>([]);
  let rolling = $state(0);
  let rollsLeft = $state(num(a.rerolls, 0, 0, 5) + 1);
  const shown = $derived(rolling > t ? Array.from({ length: dN }, (_, i) => 1 + (Math.floor(t * 23 + i * 7) % dS)) : rolls);
  function roll() {
    if (rollsLeft <= 0) return;
    rollsLeft--;
    const buf = new Uint32Array(dN);
    crypto.getRandomValues(buf);
    rolls = [...buf].map((v) => 1 + (v % dS));
    rolling = t + 0.9;
  }
  const total = $derived(rolls.reduce((s, v) => s + v, 0) + dK);

  // --- combo: turn the dials ---------------------------------------------------------------
  const symbols = L("symbol").length ? L("symbol") : ((a.symbols ?? "0123456789") as string).includes(",") ? (a.symbols as string).split(",").map((s) => s.trim()) : [...((a.symbols ?? "0123456789") as string)];
  const dials = num(a.length, 4, 1, 8);
  let dial = $state<number[]>(Array(dials).fill(0));
  const answer = a.answer ? String(a.answer).split(a.answer.includes(",") ? "," : "").map((s: string) => s.trim()) : null;
  let triesLeft = $state(num(a.tries, 3, 1, 20));
  let comboMsg = $state("");
  const comboText = $derived(dial.map((i) => symbols[i]).join(" "));
  function turn(i: number, d: number) { dial[i] = (dial[i] + d + symbols.length) % symbols.length; }
  function tryCombo() {
    if (!answer) return done(`I set the ${title} to: ${comboText}`);
    triesLeft--;
    const right = dial.every((v, i) => symbols[v] === answer[i]);
    if (right) return done(`I set the ${title} to ${comboText} and it opens.`);
    comboMsg = triesLeft ? `Nothing happens. ${triesLeft} tr${triesLeft === 1 ? "y" : "ies"} left.` : "";
    if (!triesLeft) done(`I try the ${title} and can't open it (last try: ${comboText}).`);
  }

  // --- choose: pick one or more ------------------------------------------------------------
  const options = L("option").length ? L("option") : L("card");
  const pickN = num(a.pick, 1, 1, Math.max(1, options.length));
  let picked = $state<string[]>([]);
  function toggle(o: string) {
    picked = picked.includes(o) ? picked.filter((x) => x !== o) : pickN === 1 ? [o] : picked.length < pickN ? [...picked, o] : picked;
  }

  // --- haggle: name a price and watch their face -------------------------------------------
  const ask = Math.max(1, Number(a.ask) || 50);
  const cur = a.currency ?? "gold";
  let offer = $state(Math.round(ask * 0.8));
  const mood = $derived(offer >= ask ? "😄" : offer >= ask * 0.85 ? "🙂" : offer >= ask * 0.65 ? "😐" : offer >= ask * 0.45 ? "😠" : "🤬");

  // --- sequence: watch, then repeat --------------------------------------------------------
  const seqSyms = L("symbol").length ? L("symbol") : ["🔥", "💧", "🌿", "⚡", "🌙", "☀"].slice(0, 3 + Math.min(3, difficulty));
  const seqLen = num(a.length, 3 + difficulty, 2, 12);
  const seqR = seeded(title + "seq");
  const seq = Array.from({ length: seqLen }, () => seqSyms[Math.floor(seqR() * seqSyms.length)]);
  let seqStart = $state(-1);
  let seqIn = $state<string[]>([]);
  const step = 0.9 - difficulty * 0.1;
  const flashing = $derived(seqStart >= 0 && t - seqStart < seqLen * step ? Math.floor((t - seqStart) / step) : -1);
  const seqRight = $derived(seqIn.filter((s, i) => s === seq[i]).length);
  $effect(() => {
    if (seqIn.length === seqLen) done(`I recall the sequence for ${title}: ${seqRight} of ${seqLen} right${seqRight === seqLen ? ", perfectly" : ""}.`);
  });

  // --- aim: hit the moving target ----------------------------------------------------------
  const shots = num(a.shots, 3, 1, 10);
  let hits = $state<number[]>([]);
  const target = $derived({ x: 50 + Math.sin(t * (0.7 + difficulty * 0.3)) * 38, y: 50 + Math.sin(t * (1.1 + difficulty * 0.25) + 1) * 30 });
  function shoot(e: PointerEvent) {
    if (hits.length >= shots) return;
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const x = ((e.clientX - r.left) / r.width) * 100, y = ((e.clientY - r.top) / r.height) * 100;
    const d = Math.hypot(x - target.x, (y - target.y) * (r.height / r.width));
    hits = [...hits, Math.max(0, 10 - Math.floor(d / 1.6))];
  }

  // --- arrange: put the steps in order (the GM lists them in the right order) --------------
  const steps = L("step");
  let order = $state(shuffle(steps, seeded(title + "steps")));
  function bump(i: number, d: number) { const j = i + d; if (j < 0 || j >= order.length) return; [order[i], order[j]] = [order[j], order[i]]; }
  const inPlace = $derived(order.filter((s, i) => s === steps[i]).length);

  // --- memory: match the pairs -------------------------------------------------------------
  const faces = (L("symbol").length ? L("symbol") : ["🗝", "🪙", "📜", "🧪", "🗡", "💎", "🕯", "🦴"]).slice(0, 2 + difficulty * 1.2 | 0);
  const deck = shuffle([...faces, ...faces], seeded(title + "mem"));
  let open = $state<number[]>([]);
  let matched = $state<number[]>([]);
  let moves = $state(0);
  function flip(i: number) {
    if (open.length === 2 || open.includes(i) || matched.includes(i)) return;
    open = [...open, i];
    if (open.length === 2) {
      moves++;
      const [x, y] = open;
      setTimeout(() => {
        if (deck[x] === deck[y]) matched = [...matched, x, y];
        open = [];
        if (matched.length === deck.length) done(`I match every pair in ${title} in ${moves} moves (best possible: ${faces.length}).`);
      }, 650);
    }
  }
</script>

<div class="mg col">
  {#if a.type === "lockpick"}
    <p class="small muted">Press <b>Set pin</b> when the pick is inside the gold zone. {pins} pin{pins > 1 ? "s" : ""}, {picks} pick{picks === 1 ? "" : "s"} left.</p>
    <div class="mg-bar">
      <div class="mg-zone" style:left="{zoneAt * 100}%" style:width="{zone * 100}%"></div>
      <div class="mg-pick" style:left="{pickPos * 100}%"></div>
    </div>
    <div class="row">{#each Array(pins) as _, i}<span class="mg-pin" class:on={i < set}>{i < set ? "▲" : "△"}</span>{/each}<span class="grow muted">{lockMsg}</span></div>
    <div class="row">
      <button class="primary" disabled={lockOver} onclick={tryPin}>Set pin</button>
      <span class="grow"></span>
      <button onclick={() => done(`I pick the ${title}: ${set} of ${pins} pins set, ${num(a.picks, 3, 1, 9) - picks} pick${num(a.picks, 3, 1, 9) - picks === 1 ? "" : "s"} broken${set >= pins ? ". It opens." : picks <= 0 ? ", out of picks." : ", and I stop there."}`)}>{lockOver ? "Send to the GM" : "Stop here"}</button>
    </div>
  {:else if a.type === "dice"}
    <p class="small muted">Rolling {diceSpec}{a.reason ? ` for ${a.reason}` : ""}.</p>
    <div class="row mg-dice">{#each shown.length ? shown : Array(dN).fill("?") as v}<span class="mg-die" class:spin={rolling > t}>{v}</span>{/each}{#if dK}<span class="mg-mod">{dK > 0 ? "+" : ""}{dK}</span>{/if}</div>
    {#if rolls.length && rolling <= t}<p style="font-size:1.4rem;margin:0">Total: <b>{total}</b></p>{/if}
    <div class="row">
      <button class="primary" disabled={rollsLeft <= 0 || rolling > t} onclick={roll}>🎲 {rolls.length ? `Reroll (${rollsLeft} left)` : "Roll"}</button>
      <span class="grow"></span>
      <button disabled={!rolls.length || rolling > t} onclick={() => done(`I roll ${diceSpec}${a.reason ? ` for ${a.reason}` : ""}: ${rolls.join(" + ")}${dK ? ` ${dK > 0 ? "+" : "-"} ${Math.abs(dK)}` : ""} = ${total}.`)}>Send to the GM</button>
    </div>
  {:else if a.type === "combo"}
    <div class="row mg-dials">
      {#each dial as v, i}
        <div class="col mg-dial"><button onclick={() => turn(i, 1)} aria-label="Up">▲</button><span>{symbols[v]}</span><button onclick={() => turn(i, -1)} aria-label="Down">▼</button></div>
      {/each}
    </div>
    {#if comboMsg}<p class="muted">{comboMsg}</p>{/if}
    <div class="row"><span class="grow"></span><button class="primary" onclick={tryCombo}>Try it</button></div>
  {:else if a.type === "choose"}
    <p class="small muted">Choose {pickN === 1 ? "one" : `${pickN}`}.</p>
    <div class="mg-cards">
      {#each options as o}<button class="mg-card" class:on={picked.includes(o)} onclick={() => toggle(o)}>{o}</button>{/each}
    </div>
    <div class="row"><span class="grow"></span><button class="primary" disabled={picked.length !== Math.min(pickN, options.length)} onclick={() => done(`I choose: ${picked.join("; ")}.`)}>Choose</button></div>
  {:else if a.type === "haggle"}
    <p>{a.who ?? "They"} want{a.who ? "s" : ""} <b>{ask} {cur}</b>{a.item ? ` for ${a.item}` : ""}.</p>
    <div class="row" style="font-size:2.6rem;justify-content:center">{mood}</div>
    <input type="range" min="0" max={Math.round(ask * 1.5)} bind:value={offer} aria-label="Your offer" />
    <div class="row"><span>Your offer: <b>{offer} {cur}</b></span><span class="grow"></span><button class="primary" onclick={() => done(`I offer ${offer} ${cur}${a.item ? ` for ${a.item}` : ""} (they asked ${ask}).`)}>Make the offer</button></div>
  {:else if a.type === "sequence"}
    <div class="mg-flash">{flashing >= 0 ? seq[flashing] : seqStart < 0 ? "?" : seqIn.length < seqLen ? "Your turn" : "✓"}</div>
    {#if seqStart < 0}
      <div class="row"><button class="primary" onclick={() => (seqStart = t)}>Watch the sequence ({seqLen})</button></div>
    {:else}
      <div class="row">{#each seqIn as s}<span class="mg-pin">{s}</span>{/each}</div>
      <div class="row">{#each seqSyms as s}<button class="mg-sym" disabled={flashing >= 0 || seqIn.length >= seqLen} onclick={() => (seqIn = [...seqIn, s])}>{s}</button>{/each}</div>
    {/if}
  {:else if a.type === "aim"}
    <p class="small muted">Click the target. {shots - hits.length} shot{shots - hits.length === 1 ? "" : "s"} left.</p>
    <div class="mg-range" role="button" tabindex="0" aria-label="Shooting range" onpointerdown={shoot}>
      <div class="mg-target" style:left="{target.x}%" style:top="{target.y}%"></div>
    </div>
    <div class="row">
      {#each hits as h}<span class="mg-pin">{h}</span>{/each}
      <span class="grow"></span>
      <button class="primary" disabled={hits.length < shots} onclick={() => done(`I take ${shots} shot${shots > 1 ? "s" : ""} at ${title}: ${hits.join(", ")} (${hits.reduce((s, v) => s + v, 0)} of ${shots * 10}).`)}>Send to the GM</button>
    </div>
  {:else if a.type === "arrange"}
    <p class="small muted">Put these in the right order.</p>
    {#each order as s, i}
      <div class="row mg-step"><b>{i + 1}.</b><span class="grow">{s}</span><button class="ghost small" onclick={() => bump(i, -1)} aria-label="Move up">▲</button><button class="ghost small" onclick={() => bump(i, 1)} aria-label="Move down">▼</button></div>
    {/each}
    <div class="row"><span class="grow"></span><button class="primary" onclick={() => done(`I put ${title} in this order: ${order.map((s, i) => `${i + 1}. ${s}`).join("; ")}. (${inPlace} of ${steps.length} in the right place.)`)}>This is the order</button></div>
  {:else if a.type === "memory"}
    <p class="small muted">Find the pairs. Moves: {moves}</p>
    <div class="mg-mem">
      {#each deck as f, i}<button class="mg-tile" class:up={open.includes(i) || matched.includes(i)} onclick={() => flip(i)}>{open.includes(i) || matched.includes(i) ? f : "?"}</button>{/each}
    </div>
  {/if}
</div>

<style>
  .mg { padding: 12px 14px; gap: 12px; overflow-y: auto; flex: 1; min-height: 0; }
  .mg-bar { position: relative; height: 44px; border-radius: 8px; background: #2a2d33; overflow: hidden; }
  .mg-zone { position: absolute; top: 0; bottom: 0; background: #d4a52c; opacity: 0.8; }
  .mg-pick { position: absolute; top: -4px; bottom: -4px; width: 4px; margin-left: -2px; background: #e8ecef; box-shadow: 0 0 6px #fff; }
  .mg-pin { font-size: 1.4rem; min-width: 1.6rem; text-align: center; }
  .mg-pin.on { color: #d4a52c; }
  .mg-dice { gap: 10px; flex-wrap: wrap; }
  .mg-die { width: 56px; height: 56px; display: grid; place-items: center; font-size: 1.5rem; font-weight: 800; border-radius: 10px; background: #f4f1ea; color: #222; box-shadow: inset 0 -4px 0 #0003; }
  .mg-die.spin { animation: mg-spin 0.25s linear infinite; }
  .mg-mod { font-size: 1.5rem; align-self: center; }
  @keyframes mg-spin { from { transform: rotate(0); } to { transform: rotate(360deg); } }
  .mg-dials { gap: 10px; justify-content: center; }
  .mg-dial { align-items: center; gap: 4px; }
  .mg-dial span { font-size: 2rem; min-width: 3rem; text-align: center; padding: 6px; border-radius: 8px; background: var(--panel-2); }
  .mg-cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 8px; }
  .mg-card { min-height: 90px; white-space: normal; border: 2px solid var(--line); }
  .mg-card.on { border-color: #d4a52c; background: rgba(212, 165, 44, 0.18); }
  .mg-flash { font-size: 3rem; text-align: center; min-height: 4.5rem; }
  .mg-sym { font-size: 1.8rem; min-width: 3.2rem; }
  .mg-range { position: relative; height: 260px; border-radius: 8px; background: radial-gradient(#4a6a3a, #263a1e); cursor: crosshair; touch-action: none; }
  .mg-target { position: absolute; width: 56px; height: 56px; margin: -28px 0 0 -28px; border-radius: 50%; pointer-events: none;
    background: radial-gradient(#e33 0 18%, #fff 18% 36%, #e33 36% 54%, #fff 54% 72%, #e33 72%); }
  .mg-step { padding: 6px 8px; border-radius: 6px; background: var(--panel-2); }
  .mg-mem { display: grid; grid-template-columns: repeat(auto-fill, minmax(64px, 1fr)); gap: 8px; }
  .mg-tile { height: 64px; font-size: 1.8rem; }
  .mg-tile.up { background: var(--panel-2); }
</style>
