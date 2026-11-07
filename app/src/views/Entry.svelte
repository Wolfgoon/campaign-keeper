<script lang="ts">
  import { md, safeSvg } from "../lib/api";

  let { e, speak = null } = $props();

  const good = (t: string) => t === "decisive" || t === "success" || t === "cost";
  const TIER: Record<string, string> = { decisive: "Decisive success", success: "Success", cost: "Success at a cost", failure: "Failure", disaster: "Disaster" };
  const noteClass = (c: string) => (c.startsWith("Refused") ? "no" : /LEVEL UP|SKILL UP/.test(c) ? "up" : "");
  const sign = (n: number) => (n > 0 ? `+${n}` : `${n}`);

  const unesc = (s: string) => s.replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&amp;/g, "&");

  /** What everyone did this round, so the whole table can read it. */
  function roundLines(xml: string) {
    const out: { who: string; what: string }[] = [];
    for (const m of xml.matchAll(/<(action|absent|narration)\b([^>]*?)(\/?)>(?:([\s\S]*?)<\/\1>)?/g)) {
      const name = unesc(m[2].match(/(?:character|by)="([^"]*)"/)?.[1] ?? "");
      if (m[1] === "absent") out.push({ who: name, what: "is away" });
      else if (m[3]) out.push({ who: name, what: "holds back" });
      else out.push({ who: name, what: unesc(m[4] ?? "").replace(/<drawing[\s\S]*<\/drawing>/, "(a drawing)") });
    }
    return out;
  }
</script>

{#if e.role === "user"}
  {#if e.label && e.label !== "Round" && e.text !== e.label && e.text.startsWith("/load")}
    <div class="msg-note">{e.label}</div>
  {:else if e.text.startsWith("<round>")}
    <div class="round">
      {#each roundLines(e.text) as l}<div><b>{l.who}</b> {l.what}</div>{/each}
    </div>
  {:else if e.text.startsWith("<gm_private>")}
    <div class="msg-you">🔒 {e.text.replace(/<\/?gm_private>/g, "")}</div>
  {:else}
    <div class="msg-you">{e.text}</div>
  {/if}
  {#each e.rpg?.checks ?? [] as c}
    <div class="sys">
      <b>[ CHECK ] {c.skill}</b>{#if c.subject} · {c.subject}{/if}
      <div>
        {c.score} vs {c.target} ({c.target_label}):
        <span class="tier" class:good={good(c.tier)} class:bad={!good(c.tier)}>{TIER[c.tier]}</span>
        {#if c.reused}<span class="why"> already tried; nothing has changed</span>{/if}
      </div>
      <div class="why">
        {c.attribute} {c.attribute_value}, rank {c.rank} ({sign(c.rank * 2)}){#if c.gear}, {c.gear_item} ({sign(c.gear)}){/if}{#if c.conditions}, conditions ({sign(c.conditions)}){/if}{#each c.circumstances as x}, {x.label} ({sign(x.value)}){/each}
      </div>
      {#if c.reason}<div class="why">{c.reason}</div>{/if}
      {#if c.effects?.length}
        <div class="sys-notes" style="margin-top:4px">
          {#each c.effects as f}<span class={noteClass(f)}>{f}</span>{/each}
        </div>
      {/if}
    </div>
  {/each}
  {#if e.recalled?.length}
    <details class="recall">
      <summary>🧠 Recalled {e.recalled.length} {e.recalled.length === 1 ? "memory" : "memories"}</summary>
      <ul>{#each e.recalled as r}<li>{r}</li>{/each}</ul>
    </details>
  {/if}
{:else if e.role === "assistant"}
  <div class="msg-gm">{@html md(e.text)}</div>
  {#each e.pictures ?? [] as p}
    <figure class="pic" style="margin-inline:0">
      {@html safeSvg(p.svg)}
      {#if p.title}<figcaption>{p.title}</figcaption>{/if}
    </figure>
  {/each}
  {#if e.rpg?.changes?.length}
    <div class="sys-notes">{#each e.rpg.changes as c}<span class={noteClass(c)}>{c}</span>{/each}</div>
  {/if}
  {#if e.saved_as}<div class="msg-note">Saved to {e.saved_as}</div>{/if}
  {#if speak}
    <div><button class="ghost small" onclick={() => speak(e)}>💬 Read aloud</button></div>
  {/if}
{:else if e.label}
  <div class="msg-note">{e.label}</div>
{/if}
