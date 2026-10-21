# [Campaign Title]: Campaign Folder

Everything for this campaign is kept here, so you can save the story and pick it up again in a new chat.

## Starting a new campaign from this template
1. Copy the whole `TEMPLATE` folder and rename the copy after the campaign or character (e.g. `Mira Ashdown`).
2. Replace `[Campaign Title]` at the top of this README.
3. Play the setup in a new chat (see below). After the opening scene, fill in:
   - `00_GM/settings.md` (setting, tone, difficulty, length, currency)
   - `01_Character/`: copy `_TEMPLATE.md` to `firstname_lastname.md`
   - `02_World/world_overview.md`
4. Fill in everything else as the story produces it. Files starting with `_` are blank forms to copy. Leave them unchanged.

## Folder layout
| Folder | What goes in it |
|---|---|
| `00_GM/` | GM instructions, current settings, Author's Note |
| `01_Character/` | The player character's sheet (copy `_TEMPLATE.md`) |
| `02_World/` | World Info: setting overview, locations, factions, items & lore |
| `03_NPCs/` | One file per important NPC (copy `_TEMPLATE.md`); small parts go in `minor_npcs.md` |
| `04_Quests/` | Active and completed quests and open threads |
| `05_Memory/` | Pinned Memory: facts the GM must always keep true |
| `06_Sessions/` | A log for each play session (copy `_TEMPLATE.md`) |
| `07_Maps/` | Known locations and how they connect |
| `08_Saves/` | SAVE BLOCKs from `/save`, one file per save |

Campaign Keeper keeps its chat history, backups, and trash in a hidden `.keeper/` folder here.

## Naming conventions
- NPCs, locations, factions, lore: `lowercase_with_underscores.md` (e.g. `innkeeper_tobin.md`, `the_old_mill.md`).
- Groups of NPCs who act together can share one file (e.g. `town_guard.md`).
- Sessions: `day#_short_title.md` (e.g. `day1_the_crossroads.md`), or `session#_short_title.md`.
- Saves: `YYYY-MM-DD_save_##.md` (e.g. `2026-10-07_save_01.md`).

## Starting or resuming a game
**With Campaign Keeper** (the app in `campaign_keeper/`): drop this folder onto the app and use the Play tab. It loads the GM instructions, your files, and the latest save for you. The steps below are for playing in a regular chat instead.

1. Open a new chat.
2. Paste `00_GM/ai_dungeon_plus_instructions.txt`.
3. **New game:** answer the setup questions.
   **Resume:** type `/load`, then paste the newest file from `08_Saves/`.
   For a fuller resume, also paste `00_GM/settings.md`, `05_Memory/pinned_memory.md`, and any World/NPC files that matter for the next scene.

## After each session
- [ ] Run `/save` and paste the result into a new file in `08_Saves/`.
- [ ] Add a session log in `06_Sessions/`.
- [ ] Update the character sheet: condition, inventory, **money**, relationships.
- [ ] Update or add NPC files (and `minor_npcs.md`).
- [ ] Move finished quests to `completed_quests.md`. Add new leads and mysteries.
- [ ] Add new places to `07_Maps/map.md` and `02_World/Locations/`.
- [ ] Copy important letters/inscriptions word-for-word into `02_World/Items_and_Lore/key_documents.md`.
- [ ] Add any new `/remember` facts or continuity anchors to `05_Memory/pinned_memory.md`.
