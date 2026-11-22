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

```json
{ "client_id": "1234-abcd.apps.googleusercontent.com", "client_secret": "GOCSPX-..." }
```

With the file left blank (as it is in this repo, so the shipped builds have no client of their own) the account card is hidden, and a client can be pasted in at run time under **Settings > Account > Google sign-in setup**, which overrides the built-in one. That's the way to try it without rebuilding.

### Making the client (once, free, about ten minutes)

Sign in to <https://console.cloud.google.com/> with the same Google account you'll sign in with, use the project picker at the top to **New project**, then work down this list. Everything is under the left menu's **Google Auth Platform** section, which appears when the project has one; the console asks for **Branding** first if it has none.

1. **Enable the Drive API.** Open <https://console.cloud.google.com/flows/enableapi?apiid=drive.googleapis.com> and press **Enable**. Without it every call fails with *"Google Drive API has not been used in project … or it is disabled"*.
2. **Branding.** *Google Auth Platform → Branding*: an app name (`Campaign Keeper`) and your address as user support email and developer contact. If it asks for the user type, choose **External**.
3. **Data Access.** *Google Auth Platform → Data Access → Add or remove scopes*: filter for `drive.file`, tick **`https://www.googleapis.com/auth/drive.file`** ("See, edit, create and delete only the specific Google Drive files you use with this app"), **Update**, **Save**. `openid` and `email` come along automatically and need nothing. Skipping this makes the device page answer **Error 400: invalid_scope**.
4. **Test user, while the app is in Testing.** *Google Auth Platform → Audience → Test users → Add users*: the account you sign in with. Without it the device page says *"Access blocked: Campaign Keeper has not completed the Google verification process"*.
5. **Client.** *Google Auth Platform → Clients → Create client*: application type **TVs and Limited Input devices**. This one matters — Google's device endpoint only accepts that type, so a **Desktop app**, **Web application** or **Android** client comes back `401 invalid_client` however right the secret is. (An Android client's *package name and SHA-1 fingerprint* belong to the Android sign-in SDK, which this app doesn't use; the same device client is pasted into the phone's own Settings.) Name it, **Create**, then copy the **Client ID** (ends `.apps.googleusercontent.com`) and the **Client secret**.
6. **Paste it in.** Settings → Account → **Google sign-in setup (advanced)** → both boxes → **Save and check**; the Account card appears. Press **Sign in with Google…**, then type the code it shows at <https://www.google.com/device> in any browser, and approve. The "Google hasn't verified this app" screen (unverified app in production) is expected for a client you made yourself: **Advanced → Go to Campaign Keeper**.

**Testing vs. published.** Testing is fine and needs no review, but Google expires the refresh token after **7 days**, so each device has to sign in again ("Your Google sign-in has expired. Sign in again."). Pressing **Publish app** under *Audience* keeps sign-ins until revoked; an unverified public app is capped at 100 users and shows the warning screen, which is fine for a table. A Google Workspace **Internal** app skips both the review and the cap.

### When sign-in doesn't work

| The app says | What it is |
|---|---|
| Google sign-in isn't set up yet. Add a Google client under Account in Settings. | No client: `google_oauth.json` is blank and nothing was pasted. |
| Google refused the sign-in (401): `invalid_client` / `unauthorized_client` | Wrong client type (make a **TVs and Limited Input devices** one), or the secret was copied short, or the ID belongs to another project than the secret. |
| Google refused the sign-in (400): `invalid_scope` | Step 3 wasn't done for this client's project. |
| Google Drive reported an error (403): … has not been used in project … or it is disabled | Step 1: the Drive API isn't enabled. |
| Google Drive reported an error (403): `insufficientPermissions` / *The granted scopes do not give access* | The account signed in before the scope was added. Remove the app at <https://myaccount.google.com/permissions> and sign in again. |
| Your Google sign-in has expired. Sign in again. | Testing mode's 7-day expiry, a password change, or access revoked in the Google account. |
| The code ran out. Start the sign-in again. | The device code lasts about 15 minutes. |
| Access blocked: … has not completed the Google verification process | Step 4 (test user), or publish the app. |

The client is only an ID and secret for *asking*; each player signs in with their own Google account and their stories live in their own Drive. `drive.file` also means the folder holds only files the app created — a story dropped into Drive by hand, or synced by the Drive desktop app from another machine, is invisible to it. Signing in is per device (the token is in `account.json` in the config folder), so a phone and a PC signed into the same account see the same stories.

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

## The world's own words

The AD System's vocabulary belongs to the GM, who fits it to the story: `rename amplitude -> Aether`, `rename skills -> Talents`, `rename class -> Calling`, and the rest of the list in `prompts/litrpg_ad.md`. Only the shown name changes — every number and ruling stays. The status window, the player's sheet and the panel's own headings all print the new words, and a renamed word works as a keyword in `<rpg>` lines too (`Calling offer forager`).

When the rules are turned on in a world that has already begun, the app tells the GM so in `<world_words>`, and the GM must ask the table what this world calls those things before the scene goes on. It then writes the `rename` lines itself. Classic asks in its own words (attributes, HP, Stamina, Mana, Money, skills), the AD System in its own. `rename settled` (or the "The words are settled" button in the status panel) ends the asking when the table keeps the sheet's words.

## Memory

- Live database: on this computer, outside any synced folder. `.keeper/memory-backup.jsonl` travels with the campaign, and the database rebuilds from it.
- Search: SQLite FTS5 plus bge-small-en-v1.5 embeddings (pure Rust, downloaded once), merged by reciprocal rank fusion. Word search works if the model isn't available.
- Nothing is deleted by condensing: entries stay in `chat.json`, and only what's sent to the model changes.
- The Keeper reads only what the player has seen, never the GM's notebook.

## Not compatible with 1.x

Multiplayer's wire format changed. Everyone at a table needs 2.x.
