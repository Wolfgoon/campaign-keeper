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

## Android

```sh
rustup target add aarch64-linux-android
./build-android.sh        # from Git Bash: writes installers/Campaign Keeper <version> (Android).apk
```

Needs Android Studio (SDK and Java) and the Android NDK under `Android/Sdk/ndk`. The APK is signed with the key named in `src-tauri/gen/android/keystore.properties` (not committed). Keep that key file and its passwords backed up: Android only accepts an update signed with the same key. Windows needs Developer Mode for Tauri's own `tauri android build`; the script works without it.

On a phone or tablet the app keeps campaigns in its own storage, has no Claude Code or Antigravity (those are desktop programs), and lays the Play screen out in one column.

## iPhone, iPad and Mac

These can only be built on a Mac. `.github/workflows/apple.yml` builds both on GitHub's Macs: run "Apple builds" from the repository's Actions tab and download the results from the finished run. The iPhone/iPad app comes out unsigned; see the notes at the top of that file.

## Joining from a web browser

A table can also serve the player's screen as a web page (`net/web.rs`, port 47122), so a friend joins from any browser with nothing installed. It's the same Svelte build: outside Tauri, `lib/api.ts` routes the player screen's calls to `lib/web.ts`, which talks to the host over a WebSocket. Browser players get the same seat, rules and limits as app players (`table::seat`), including the wrong-code lockout. The connection is plain HTTP and WebSocket, not the app's encrypted channel, and the host can leave it off.

## Accounts and sync

Players sign in with Google, and each story is kept as one zip in a "Campaign Keeper" folder in their own Google Drive (`account.rs`, `sync.rs`). The app asks for the `drive.file` scope, so it can only see files it made.

To turn accounts on in a build, put a Google OAuth client in `src-tauri/google_oauth.json`:

1. In the [Google Cloud console](https://console.cloud.google.com/), make a project and enable the **Google Drive API**.
2. Under **Google Auth Platform**, set up the consent screen (External), add the scope `.../auth/drive.file`, and publish the app ("In production"). While it's in "Testing", only listed test users can sign in and sign-ins expire after a week.
3. Under **Clients**, create an OAuth client of type **TVs and Limited Input devices**. Copy its client ID and client secret into `google_oauth.json`.

With the file left blank the account card is hidden, and a client can still be pasted in at run time under **Settings > Account > Google sign-in setup**, which overrides the built-in one.

How sync decides: it compares this device's files, the account's copy, and what both were when they last matched. If only one side changed, that side is copied across. If both changed, the player chooses, and the files being replaced are zipped into a backup on the device first. It runs when a campaign opens, after each GM reply, and when it closes.

## What's in it

| Part | Where |
|---|---|
| Turn loop | `src-tauri/src/game.rs` |
| LitRPG rules (no randomness) | `rpg.rs`, `referee.rs` |
| Memory: store, keeper, condenser, recall, playstyle | `memory/` |
| Engines: Claude Code, Claude API, OpenAI-compatible | `engine/` |
| Scribe | `scribe.rs` |
| Multiplayer | `net/` (whole-table scenario tests in `net/table_tests.rs`) |
| Accounts and sync | `account.rs`, `sync.rs` |
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
