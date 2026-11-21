<script lang="ts">
  import { call, toast } from "../lib/api";

  // `rules` is the host's full state; a player at someone's table gets just `sheet`, read-only.
  // `only` keeps a host who is playing a character at their own table to that one sheet; a null
  // `only` means no limit (the GM reading every sheet).
  let {
    rules = $bindable(null),
    sheet = null,
    readOnly = false,
    onSpend = null,
    labels = null,
    only = null,
  }: { rules?: any; sheet?: any; readOnly?: boolean; onSpend?: any; labels?: any; only?: string | null } = $props();

  // What this story calls things.
  const named = $derived(labels ?? rules?.labels ?? {});
  const L = (n: string) => named[n] ?? n;
  let working = $state(false);

  const ATTRS = ["Might", "Agility", "Endurance", "Intellect", "Insight", "Presence"];
  const RANKS = (r: number) => (r <= 0 ? "Untrained" : r === 1 ? "Novice" : r <= 3 ? "Trained" : r <= 5 ? "Expert" : r <= 7 ? "Master" : r <= 9 ? "Grandmaster" : "Legend");
  const xpFor = (level: number) => 50 * (level - 1) * level;
  const pct = (a: number, b: number) => (b > 0 ? Math.max(0, Math.min(100, (a / b) * 100)) : 0);

  const mine = (s: any) => only === null || only === undefined || String(s?.name ?? "").toLowerCase() === only.toLowerCase();
  let sheets = $derived((sheet ? [sheet] : Object.values(rules?.sheets ?? {})) as any[]);
  const shown = $derived(sheets.filter(mine));

  // The AD System (Spirit's rules): the host's sheets come ready-worked from the app.
  const isAd = $derived(sheet ? sheet.ruleset === "ad" : rules?.ruleset === "ad");
  let adViews = $state<any[]>([]);
  $effect(() => {
    if (!sheet && rules?.enabled && rules?.ruleset === "ad") {
      void rules.log?.length;
      call("rpg_views").then((v: any) => (adViews = v)).catch(() => {});
    }
  });
  const adSheets = $derived(sheet ? [sheet] : adViews.filter(mine));
  let ruleset = $state("ad");
  // The panel's own words take the story's names: labels come keyed lowercase from the app.
  const N = (s: any, k: string) => s?.labels?.[k] ?? s?.labels?.[k.toLowerCase()] ?? k;

  async function settle() {
    try {
      rules = await call("rpg_terms");
    } catch (e) {
      toast(e);
    }
  }

  async function enable() {
    working = true;
    try {
      rules = await call("rpg_enable", { ruleset });
    } catch (e) {
      toast(e);
    }
    working = false;
  }

  const canSpend = $derived(!readOnly || !!onSpend);

  async function spend(character: string, kind: string, name: string) {
    if (onSpend) return onSpend(kind, name);
    try {
      rules = await call("rpg_spend", { character, kind, name });
    } catch (e) {
      toast(e);
    }
  }
</script>

{#if !readOnly && !rules?.enabled}
  <div class="col">
    <h3>LitRPG rules</h3>
    <p class="small muted">Levels, skills and a status window. The app does the sums, so results follow from the character and the situation. There are no dice and nothing random.</p>
    <label class="row small"><input type="radio" bind:group={ruleset} value="ad" /> <span><b>AD System</b>: the GM is the System. Levels to 15,625, class, species and skill tiers with five paths each, feats, a soul prism of frequencies with amplitude and semi skills, and crafting from the elements.</span></label>
    <label class="row small"><input type="radio" bind:group={ruleset} value="classic" /> <span><b>Classic</b>: six attributes, skill ranks, and checks the app resolves.</span></label>
    <p class="small muted">Turning this on builds a sheet from the character files. A new character gets one from the GM.</p>
    <button class="primary" disabled={working} onclick={enable}>{working ? "Building the sheet…" : "Turn on LitRPG rules"}</button>
  </div>
{:else if isAd}
  <div class="col status">
    {#if !readOnly && rules?.terms_pending}
      <div class="sect">
        <b>Words for this world</b>
        <p class="small muted">The rules came on after the story had begun, so the GM will ask the table what to call its words before the scene goes on, and write the new names itself.</p>
        <button class="ghost small" onclick={settle}>✔ The words are settled</button>
      </div>
    {/if}
    {#if !adSheets.length}
      <p class="small muted">The System is on. The status window opens once the GM has settled the character.</p>
    {/if}
    {#each adSheets as s}
      <div class="sys">
        <h3>[ {N(s, "System").toUpperCase()} ] {s.name}</h3>
        <div class="small">{s.species} ({s.species_tier}) · {s.class ? `${s.class} (${s.class_tier})` : `No ${N(s, "class").toLowerCase()}`}</div>
        <div style="margin-top:6px">{N(s, "Level")} {s.level} · {N(s, "Experience")} {s.xp} / {s.xp_next}</div>
        <div class="bar xp"><i style:width="{s.xp_percent}%"></i></div>

        <div class="sect">
          <table><tbody>
            {#each Object.entries(s.stats) as [k, v]}
              <tr title="+{s.per_level[k]} per {N(s, "level").toLowerCase()}"><td>{N(s, k)}</td><td class="n">{v}
                {#if canSpend && s.bonus_points.to_spend > 0}<button aria-label="Raise {k}" onclick={() => spend(s.name, "bp", `stat|${k}|1`)}>+</button>{/if}
              </td></tr>
            {/each}
            {#each Object.entries(s.derived) as [k, v]}<tr><td class="small">{k}</td><td class="n small mono">{v}</td></tr>{/each}
          </tbody></table>
          {#if s.bonus_points.to_spend > 0}<div class="small" style="color:var(--gold)">{s.bonus_points.to_spend} {N(s, "bonus points").toLowerCase()} to spend (+0.1% a stat each)</div>{/if}
        </div>

        <div class="sect">
          <b>{N(s, "spectrums")}</b> <span class="small muted">{s.environment}</span>
          {#each s.spectrums as f}
            <div title={f.does}>{f.number} {f.name} · {f.spectrum} <span class="small mono">{f.amplitude}</span>{#if f.blocked} <span class="why">blocked</span>{:else if f.overused_percent > 0}<span class="why"> overused {f.overused_percent.toFixed(1)}%</span>{/if}</div>
            <div class="bar mp"><i style:width="{f.percent}%"></i></div>
          {/each}
          {#if s.pain > 0}<div class="small why">Overuse pain {s.pain.toFixed(1)}</div>{/if}
        </div>

        {#each Object.entries(s.paths_open ?? {}) as [tag, opts]}
          {@const who = tag.startsWith("skill:") ? tag.slice(6) : N(s, tag)}
          <div class="sect"><b>{who} can rise a tier</b>
            {#each opts as any[] as o}
              <div><button disabled={!canSpend} onclick={() => spend(s.name, "path", `${tag}|${o.pick}`)}>{o.pick}. {o.name}{o.yours_alone ? " *" : ""}</button><div class="why">{o.about}</div></div>
            {/each}
            <div class="small muted">* yours alone</div>
          </div>
        {/each}

        {#if s.classes_available?.length}
          <div class="sect"><b>{N(s, "classes")} offered</b>
            {#each s.classes_available as c}<div><button disabled={!canSpend} onclick={() => spend(s.name, "class", c.key)}>{c.name}</button><div class="why">{c.about}</div></div>{/each}
          </div>
        {/if}

        <div class="sect">
          <b>{N(s, "skills")}</b>
          {#if !Object.keys(s.skills).length}<div class="small">None yet.</div>{/if}
          <table><tbody>
            {#each Object.entries(s.skills) as [k, v]}
              {@const sk = v as any}
              <tr title="{sk.does} {sk.abilities.map((a: any) => a.name).join(', ')}">
                <td>{sk.called}{sk.kind !== "general" ? ` (${sk.kind})` : ""}</td>
                <td class="n">{sk.level} {sk.tier}
                  {#if canSpend && s.bonus_points.to_spend >= 100}<button aria-label="Raise {k}'s tier" onclick={() => spend(s.name, "bp", `skill_tier|${k}`)}>+</button>{/if}
                </td>
              </tr>
            {/each}
          </tbody></table>
        </div>

        {#if s.feats.length || s.titles.length}
          <div class="sect"><b>{N(s, "feats")}</b>
            <ul>{#each s.feats as f}<li title={f.rewards.join(", ")}>{f.name}<div class="why">{f.about}</div></li>{/each}</ul>
            {#if s.titles.length}<div class="small">{N(s, "titles")}: {s.titles.join(", ")}</div>{/if}
          </div>
        {/if}

        {#if s.conditions.length}<div class="sect"><b>{N(s, "conditions")}</b><ul>{#each s.conditions as c}<li>{c}</li>{/each}</ul></div>{/if}

        <div class="sect"><b>{N(s, "inventory")}</b> <span class="small">({s.money} {N(s, "money").toLowerCase()})</span>
          {#if !s.inventory.length}<div class="small">Empty.</div>{/if}
          <ul>{#each s.inventory as i}<li title={i.about}>{i.name}{i.qty > 1 ? ` x${i.qty}` : ""}</li>{/each}</ul>
        </div>
      </div>
    {/each}

    {#if !readOnly && rules?.log?.length}
      <details class="recall">
        <summary>{N(adSheets[0] ?? {}, "System")} log</summary>
        <ul>{#each rules.log.slice(-30).reverse() as l}<li class="mono">{l}</li>{/each}</ul>
      </details>
    {/if}
    {#if !readOnly}
      <button class="ghost small" onclick={async () => { await call("rpg_disable"); rules = await call("rpg_state"); }}>Turn off the rules</button>
    {/if}
  </div>
{:else}
  <div class="col status">
    {#if !readOnly && rules?.terms_pending}
      <div class="sect">
        <b>Words for this world</b>
        <p class="small muted">The rules came on after the story had begun, so the GM will ask the table what to call its words before the scene goes on, and write the new names itself.</p>
        <button class="ghost small" onclick={settle}>✔ The words are settled</button>
      </div>
    {/if}
    {#if !shown.length}
      <p class="small muted">Rules are on. The status window opens once the GM has settled the character.</p>
    {/if}
    {#each shown as s}
      <div class="sys">
        <h3>[ STATUS ] {s.name}</h3>
        <div class="small">Level {s.level} {s.class || "Adventurer"}</div>
        <div style="margin-top:6px">XP {s.xp} / {xpFor(s.level + 1)}</div>
        <div class="bar xp"><i style:width="{pct(s.xp - xpFor(s.level), xpFor(s.level + 1) - xpFor(s.level))}%"></i></div>
        <div>{L("HP")} {s.hp.cur} / {s.hp.max}</div>
        <div class="bar hp"><i style:width="{pct(s.hp.cur, s.hp.max)}%"></i></div>
        <div>{L("Stamina")} {s.stamina.cur} / {s.stamina.max}</div>
        <div class="bar st"><i style:width="{pct(s.stamina.cur, s.stamina.max)}%"></i></div>
        {#if s.mana.max > 0}
          <div>{L("Mana")} {s.mana.cur} / {s.mana.max}</div>
          <div class="bar mp"><i style:width="{pct(s.mana.cur, s.mana.max)}%"></i></div>
        {/if}

        <div class="sect">
          <table><tbody>
            {#each ATTRS as a}
              <tr><td>{L(a)}</td><td class="n">{s.attributes[a] ?? 10}
                {#if canSpend && s.attribute_points > 0}<button aria-label="Raise {L(a)}" onclick={() => spend(s.name, "attribute", a)}>+</button>{/if}
              </td></tr>
            {/each}
          </tbody></table>
          {#if s.attribute_points > 0}<div class="small" style="color:var(--gold)">{s.attribute_points} attribute point{s.attribute_points > 1 ? "s" : ""} to spend</div>{/if}
        </div>

        <div class="sect">
          <b>Skills</b>
          {#if !Object.keys(s.skills).length}<div class="small">None yet.</div>{/if}
          <table><tbody>
            {#each Object.entries(s.skills) as [k, v]}
              {@const sk = v as any}
              <tr title="{L(sk.attribute)}. {sk.marks} practice marks toward the next rank.">
                <td>{k}</td>
                <td class="n">{RANKS(sk.rank)} {sk.rank}
                  {#if canSpend && s.skill_points > 0}<button aria-label="Raise {k}" onclick={() => spend(s.name, "skill", k)}>+</button>{/if}
                </td>
              </tr>
            {/each}
          </tbody></table>
          {#if s.skill_points > 0}<div class="small" style="color:var(--gold)">{s.skill_points} skill point{s.skill_points > 1 ? "s" : ""} to spend</div>{/if}
        </div>

        {#if s.conditions.length}
          <div class="sect"><b>Conditions</b>
            <ul>{#each s.conditions as c}<li>{c.name} ({c.modifier > 0 ? "+" : ""}{c.modifier})</li>{/each}</ul>
          </div>
        {/if}

        <div class="sect"><b>Inventory</b> <span class="small">({s.money} {L("Money").toLowerCase()})</span>
          {#if !s.inventory.length}<div class="small">Empty.</div>{/if}
          <ul>
            {#each s.inventory as i}
              <li>{i.name}{i.qty > 1 ? ` x${i.qty}` : ""}
                {#each Object.entries(i.bonuses ?? {}) as [k, b]}<span class="why"> {(b as number) > 0 ? "+" : ""}{b} {k}</span>{/each}
              </li>
            {/each}
          </ul>
        </div>

        {#if Object.keys(s.perks ?? {}).length}
          <div class="sect"><b>Perks</b>
            <ul>{#each Object.entries(s.perks) as [k, v]}<li title={String(v)}>{k}<div class="why">{v}</div></li>{/each}</ul>
          </div>
        {/if}
      </div>
    {/each}

    {#if !readOnly && rules?.log?.length}
      <details class="recall">
        <summary>System log</summary>
        <ul>{#each rules.log.slice(-30).reverse() as l}<li class="mono">{l}</li>{/each}</ul>
      </details>
    {/if}
    {#if !readOnly}
      <button class="ghost small" onclick={async () => { await call("rpg_disable"); rules = await call("rpg_state"); }}>Turn off the rules</button>
    {/if}
  </div>
{/if}
