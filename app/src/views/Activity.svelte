<script lang="ts">
  import { safeSvg } from "../lib/api";

  // A hands-on tab the GM opened: drawing, crafting or writing. The result goes back as an action.
  let { a, submit, close } = $props();

  const inks = $derived.by(() => {
    const raw = (a.inks ?? "Ink:#2b2e33") as string;
    return raw.split(",").map((p) => {
      const [n, c] = p.split(":").map((x) => x.trim());
      return { name: c ? n : "Ink", color: c ?? n };
    });
  });
  const surface = $derived(a.surface ?? (a.texture === "metal" ? "#8d939b" : a.texture === "stone" ? "#77756f" : a.texture === "wood" ? "#7a5a3a" : a.texture === "slate" ? "#3a3f44" : a.texture === "sand" ? "#d9c79a" : a.texture === "cloth" ? "#c9bfa8" : a.texture === "plain" ? "#ffffff" : "#efe4c8"));
  const dash = $derived(a.stroke === "dashed" ? "10 7" : a.stroke === "dotted" ? "1 7" : undefined);
  const [lo, hi] = $derived(((a.width ?? "2-4") as string).split("-").map(Number));

  let ink = $state(0);
  let size = $state(3);
  let strokes = $state<{ d: string; color: string; w: number }[]>([]);
  let cur: { d: string; color: string; w: number } | null = null;
  let text = $state("");
  let how = $state("");
  let making = $state("");
  let chosen = $state<string[]>([]);
  let extra = $state("");

  let canvas: SVGSVGElement;
  function pt(e: PointerEvent) {
    const r = canvas.getBoundingClientRect();
    return `${Math.round(((e.clientX - r.left) / r.width) * 480)} ${Math.round(((e.clientY - r.top) / r.height) * 320)}`;
  }
  function down(e: PointerEvent) {
    canvas.setPointerCapture(e.pointerId);
    cur = { d: `M${pt(e)}`, color: inks[ink]?.color ?? "#222", w: size };
    strokes.push(cur);
    cur = strokes[strokes.length - 1];
  }
  function move(e: PointerEvent) {
    if (cur) cur.d += ` L${pt(e)}`;
  }

  function send() {
    const title = a.title ?? "Activity";
    if (a.type === "drawing") {
      const paths = strokes.map((s) => `<path d="${s.d}" fill="none" stroke="${s.color}" stroke-width="${s.w}" stroke-linecap="round" stroke-linejoin="round"/>`).join("");
      const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 480 320">${paths}</svg>`;
      if (svg.length > 400_000) return;
      submit(`I finish the drawing: ${title}.${text ? ` ${text}` : ""}\n<drawing title="${title}" note="The player's drawing as SVG; it's what the character made.">${svg}</drawing>`, a.id);
    } else if (a.type === "crafting") {
      const mats = [...chosen, ...(extra ? [extra] : [])].join(", ");
      submit(`I craft: ${making || title}. Materials: ${mats || "what I have"}. How: ${how}`, a.id);
    } else {
      submit(`I ${a.verb ?? "write"}:\n"""\n${text}\n"""`, a.id);
    }
  }
</script>

<div class="activity" role="dialog" aria-label={a.title}>
  <div class="row" style="padding:10px 14px 0">
    {#if a.art?.icon}<span style="width:24px;height:24px;display:inline-block">{@html safeSvg(a.art.icon)}</span>{:else if a.icon}<span>{a.icon}</span>{/if}
    <h2 style="margin:0">{a.title ?? "Activity"}</h2>
    <span class="grow"></span>
    <button class="ghost" onclick={() => close(a.id)}>Close without sending</button>
  </div>
  <p class="muted" style="margin:4px 14px 0">{a.prompt}{a.tool ? ` · with ${a.tool}` : ""}</p>

  {#if a.type === "drawing"}
    <div class="surface" style:background={surface}>
      {#if a.art?.surface}<div class="art">{@html safeSvg(a.art.surface)}</div>{/if}
      <svg class="canvas" viewBox="0 0 480 320" preserveAspectRatio="none" role="img" aria-label="Drawing surface" bind:this={canvas} onpointerdown={down} onpointermove={move} onpointerup={() => (cur = null)}>
        {#each strokes as s}
          <path d={s.d} fill="none" stroke={s.color} stroke-width={s.w} stroke-dasharray={dash} stroke-linecap="round" stroke-linejoin="round" />
        {/each}
      </svg>
    </div>
    <div class="row" style="padding:0 14px 8px">
      {#each inks as k, i}
        <button class:primary={i === ink} onclick={() => (ink = i)}><span style:color={k.color}>●</span> {k.name}</button>
      {/each}
      <input type="range" min={lo || 1} max={hi || 8} bind:value={size} style="width:110px" aria-label="Brush size" />
      <button onclick={() => strokes.pop()}>↺ Undo stroke</button>
      <button onclick={() => (strokes = [])}>Clear</button>
      <input class="grow" style="width:auto" bind:value={text} placeholder="Caption (optional): what is it?" />
    </div>
  {:else if a.type === "crafting"}
    <div class="col pad scroll grow">
      <label class="field"><span>What are you making?</span><input bind:value={making} placeholder="e.g. a working cart wheel" /></label>
      <div class="row">
        {#each [...(a.items ?? []), ...(a.tools ?? [])] as it}
          <label class="row small">
            <input type="checkbox" checked={chosen.includes(it)} onchange={() => (chosen = chosen.includes(it) ? chosen.filter((x) => x !== it) : [...chosen, it])} />
            {#if a.item_art?.[it]}<span style="width:28px;height:28px;display:inline-block">{@html safeSvg(a.item_art[it])}</span>{/if}
            {it}
          </label>
        {/each}
      </div>
      <input bind:value={extra} placeholder="Something else you have…" aria-label="Something else you have" />
      <label class="field grow"><span>How do you go about it?</span><textarea rows="6" bind:value={how} placeholder="Your character's approach, step by step…"></textarea></label>
    </div>
  {:else}
    <div class="surface" style:background={surface}>
      {#if a.art?.surface}<div class="art">{@html safeSvg(a.art.surface)}</div>{/if}
      <textarea bind:value={text} placeholder="Write it in your character's words…" aria-label="Your writing" style:color={inks[0]?.color} style:font-family={a.font === "mono" ? "var(--mono)" : undefined}></textarea>
    </div>
  {/if}
  <div class="row" style="padding:0 14px 12px"><span class="grow"></span><button class="primary" onclick={send}>Send to the GM</button></div>
</div>
