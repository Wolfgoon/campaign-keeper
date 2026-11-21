# Campaign Keeper

A desktop app for playing tabletop-style campaigns with an AI Game Master: Claude, Gemini through Antigravity, DeepSeek, or a free model online or on your own computer. It keeps your character, world, NPCs, quests, sessions and saves organized, and friends can join your game from their own computers.

## Download

| Your computer | Download |
|---|---|
| Windows 10 or 11 (64-bit) | [Campaign Keeper Setup 3.0.2.exe](https://github.com/Wolfgoon/campaign-keeper/raw/main/installers/Campaign%20Keeper%20Setup%203.0.2.exe) (version 3.0.2) |
| Android phone or tablet (Android 7+) | [Campaign Keeper 3.0.2 (Android).apk](https://github.com/Wolfgoon/campaign-keeper/raw/main/installers/Campaign%20Keeper%203.0.2%20%28Android%29.apk) (version 3.0.2) |
| iPhone, iPad, or anything else with a web browser | Nothing to download for joining a friend's game: see **Joining without the app** below |
| Mac (Apple Silicon or Intel, macOS 11+) | [Campaign Keeper 1.0.5.dmg](https://github.com/Wolfgoon/campaign-keeper/raw/main/installers/Campaign%20Keeper%201.0.5.dmg) (version 1.0.5; 2.x coming) |
| Linux or Steam Deck (Desktop Mode) | [Campaign Keeper 1.0.5 (Linux & Steam Deck).tar.gz](https://github.com/Wolfgoon/campaign-keeper/raw/main/installers/Campaign%20Keeper%201.0.5%20%28Linux%20%26%20Steam%20Deck%29.tar.gz) (version 1.0.5; 2.x coming) |

## Install

- **Mac:** open the `.dmg` and drag Campaign Keeper into Applications. The first time you open it, macOS may say it can't verify the app: click Done, then go to System Settings → Privacy & Security and click **Open Anyway**.
- **Windows:** double-click the Setup file. If Windows says "Windows protected your PC", click **More info**, then **Run anyway**. It installs just for you, with no administrator needed. When Windows Firewall asks, allow Campaign Keeper on private networks so friends can join your games.
- **Android:** open the `.apk` on the phone or tablet. Android asks whether to allow installing apps from your browser or files app: allow it, then tap Install. If Play Protect asks, choose **Install anyway**.
- **Linux / Steam Deck:** in Desktop Mode, right-click the file → Extract → Extract archive here. Open the `Campaign Keeper` folder and double-click **Campaign Keeper Setup**. Use a mouse and keyboard.

The warnings appear because the app isn't signed with a paid developer certificate. They show once per computer.

**Playing together:** everyone at a table needs version 3.0 or later, or a web browser. An older app is told to update when it tries to join.

## What you need

- **Joining a friend's game:** the app, or just a web browser (see below).
- **Hosting or playing solo:** one of these, chosen in Settings:
  - **Claude Code** with a Claude Pro or Max plan (computers only). The app sets it up for you: click **Set up Claude Code for me**, then **Sign in to Claude Code**.
  - An **Anthropic API key**.
  - **Antigravity** with your Google account (Windows 2.2.3 and later; computers only). The app sets it up for you: click **Set up Antigravity for me**, then **Sign in to Antigravity**.
  - **Other models:** pick a service from the list. Free options include Google Gemini, OpenRouter's free models, Mistral and GLM, or a model on your own computer with Ollama, LM Studio, KoboldCpp and others. DeepSeek (V4 Flash) and Xiaomi MiMo are cheap paid options.

## Joining without the app

A friend can play at your table from any web browser, with nothing to install. This is how iPhone and iPad players join.

1. Host: open the **🛠 Host** tab, choose **👥 Table**, and press **Open the table** with "Let friends join from a web browser" ticked (Windows 2.3.0 or Android 2.3.0 and later).
2. The table panel shows a link such as `http://192.168.1.20:47122`. Send it to your friend with the join code (**Copy an invitation** does both).
3. Friend: open the link, type a name and the code, and play. Adding the page to the home screen makes it open like an app.

The link works for anyone on the same Wi-Fi. A friend somewhere else needs a free VPN such as [Tailscale](https://tailscale.com) on both ends, and uses the host's Tailscale address in the link. Browser connections aren't encrypted the way the app's are, so use them at home or over a VPN.

Everyone at the table sees the same pages: the story, their own character sheet, the party's shared notes, the map, the music and table talk. Players get action buttons for what their character does or says, and can jot things on the notes page; nobody can read anyone else's sheet. The host's own tools (the table, files, the Scribe, memory and search) sit together under one **🛠 Host** tab.

## Playing on your phone with your computer's Claude Code

Claude Code runs only on computers, but your phone can play a game whose GM runs on your computer, from anywhere, over [Tailscale](https://tailscale.com):

1. Install Tailscale on the computer and the phone and sign in to the same account on both. Each device gets a Tailscale name (for example `my-pc`).
2. On the computer: set up Claude Code in Settings, open the campaign, then the **🛠 Host** tab → **👥 Table** → **Open the table**. Leave the computer on with Campaign Keeper open.
3. On the phone, open Campaign Keeper and tap **Join a friend's game…**, then paste the **invitation** the computer's table panel shows (it starts with `campaignkeeper://`): the address and the join code fill in by themselves. You can also type the computer's Tailscale name and the join code by hand, or (with "Let friends join from a web browser" ticked) open `http://my-pc:47122` in a browser.
4. Take your character and play. Every reply comes from Claude Code on the computer. Tick **Let people in without asking** on the computer to skip approving your phone each time.

The invitation is the whole message: it carries the address, the port and the join code. The table panel offers a Tailscale one when Tailscale is running, and the local-network one beside it.

## Campaign template

The `TEMPLATE` folder is a blank campaign. You don't need it to get started: the app has the same template built in (**Start a new campaign…** on its welcome screen).