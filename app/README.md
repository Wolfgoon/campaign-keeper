# Campaign Keeper 2

The Tauri + Rust rewrite of Campaign Keeper. It opens 1.x campaigns and settings as they are.

## Run and build

Needs Node 20+, Rust, and Tauri's [system prerequisites](https://tauri.app/start/prerequisites/).

```sh
npm install
npm run tauri dev                      # run it
npm run tauri build -- --bundles nsis  # Windows installer
cd src-tauri && cargo test --lib -- --test-threads=1
```

Set `CARGO_TARGET_DIR` to somewhere outside OneDrive; build output is several GB. `CK_DATA_DIR` moves all settings and data, for testing without touching a real install.

## What's in it

| Part | Where |
|---|---|
| Turn loop | `src-tauri/src/game.rs` |
| LitRPG rules (no randomness) | `rpg.rs`, `referee.rs` |
| Memory: store, keeper, condenser, recall, playstyle | `memory/` |
| Engines: Claude Code, Claude API, OpenAI-compatible | `engine/` |
| Scribe | `scribe.rs` |
| Multiplayer | `net/` |
| GM prompts | `prompts/` |
| Interface (Svelte) | `src/` |

## LitRPG rules

`score = attribute + rank x 2 + best gear bonus + conditions + circumstances (capped at ±5)`

| score minus target | Result | Failure stakes applied |
|---|---|---|
| 5 or more | Decisive success | none |
| 0 to 4 | Success | none |
| -1 to -2 | Success at a cost | half |
| -3 to -6 | Failure | full |
| -7 or less | Disaster | one and a half |

A small Referee model judges the difficulty band, circumstances and stakes; `rpg.rs` does all the arithmetic. The same subject and skill keep their first ruling, so asking again can't get an easier one. There is no random number generator in the game. The only randomness in the app is the OS generator making join codes and memory ids, which never touch play.

## Memory

- Live database: on this computer, outside any synced folder. `.keeper/memory-backup.jsonl` travels with the campaign, and the database rebuilds from it.
- Search: SQLite FTS5 plus bge-small-en-v1.5 embeddings (pure Rust, downloaded once), merged by reciprocal rank fusion. Word search works if the model isn't available.
- Nothing is deleted by condensing: entries stay in `chat.json`, and only what's sent to the model changes.
- The Keeper reads only what the player has seen, never the GM's notebook.

## Not compatible with 1.x

Multiplayer's wire format changed. Everyone at a table needs 2.x.
