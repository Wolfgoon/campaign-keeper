<script lang="ts">
  import { call, toast } from "../lib/api";

  // `rules` is the host's full state; a player at someone's table gets just `sheet`, read-only.
  let { rules = $bindable(null), sheet = null, readOnly = false, onSpend = null } = $props();
  let working = $state(false);

  const ATTRS = ["Might", "Agility", "Endurance", "Intellect", "Insight", "Presence"];
  const RANKS = (r: number) => (r <= 0 ? "Untrained" : r === 1 ? "Novice" : r <= 3 ? "Trained" : r <= 5 ? "Expert" : r <= 7 ? "Master" : r <= 9 ? "Grandmaster" : "Legend");
  const xpFor = (level: number) => 50 * (level - 1) * level;
  const pct = (a: number, b: number) => (b > 0 ? Math.max(0, Math.min(100, (a / b) * 100)) : 0);

  let sheets = $derived(sheet ? [sheet] : Object.values(rules?.sheets ?? {}) as any[]);

  async function enable() {
    working = true;
    try {
      rules = await call("rpg_enable");
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
    <p class="small muted">Levels, attributes, skills and a status window. The app does the sums, so results follow from the character and the situation. There are no dice and nothing random.</p>
    <p class="small muted">Turning this on builds a sheet from the character files. A new character gets one from the GM.</p>
    <button class="primary" disabled={working} onclick={enable}>{working ? "Building the sheet…" : "Turn on LitRPG rules"}</button>
  </div>
{:else}
  <div class="col status">
    {#if !sheets.length}
      <p class="small muted">Rules are on. The status window opens once the GM has settled the character.</p>
    {/if}
    {#each sheets as s}
      <div class="sys">
        <h3>[ STATUS ] {s.name}</h3>
        <div class="small">Level {s.level} {s.class || "Adventurer"}</div>
        <div style="margin-top:6px">XP {s.xp} / {xpFor(s.level + 1)}</div>
        <div class="bar xp"><i style:width="{pct(s.xp - xpFor(s.level), xpFor(s.level + 1) - xpFor(s.level))}%"></i></div>
        <div>HP {s.hp.cur} / {s.hp.max}</div>
        <div class="bar hp"><i style:width="{pct(s.hp.cur, s.hp.max)}%"></i></div>
        <div>Stamina {s.stamina.cur} / {s.stamina.max}</div>
        <div class="bar st"><i style:width="{pct(s.stamina.cur, s.stamina.max)}%"></i></div>
        {#if s.mana.max > 0}
          <div>Mana {s.mana.cur} / {s.mana.max}</div>
          <div class="bar mp"><i style:width="{pct(s.mana.cur, s.mana.max)}%"></i></div>
        {/if}

        <div class="sect">
          <table><tbody>
            {#each ATTRS as a}
              <tr><td>{a}</td><td class="n">{s.attributes[a] ?? 10}
                {#if canSpend && s.attribute_points > 0}<button aria-label="Raise {a}" onclick={() => spend(s.name, "attribute", a)}>+</button>{/if}
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
              <tr title="{sk.attribute}. {sk.marks} practice marks toward the next rank.">
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

        <div class="sect"><b>Inventory</b> <span class="small">({s.money} money)</span>
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
