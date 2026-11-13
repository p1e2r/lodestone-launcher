# Lodestone

A native Minecraft launcher written in Rust with [GPUI](https://gpui.rs).

> Early preview (0.1). Everything below works; expect rough edges.

## Features

- Any Minecraft version, from the oldest alpha to the newest snapshot, in offline mode
- Downloads Mojang's own Java runtimes automatically (the right Java for each version)
- Mod loaders: **Fabric, Quilt, Forge, NeoForge**, for clients and servers
- **Modrinth**: browse and install mods, resource packs and shaders (with dependencies),
  and `.mrpack` modpacks
- **Server hosting** for vanilla and every loader, with a live console and commands
- Pixel-art look: a unique generated scene for every instance, pixel fonts, blocky UI
- One-click updates from GitHub releases

## Using it

- **Library / Servers**: create instances or servers for any version and loader, or import a `.mrpack`.
- **Instance page**: Play, manage mods / resource packs / shaders, view the game log, set memory.
- **Server page**: Start/Stop, live console with commands, join address, port / MOTD / online mode / EULA.
- **Discover**: search Modrinth; pick an instance to see only compatible content.
  Installing a **modpack** always creates a **new instance** (existing instances are never
  changed), and the pack's mods are marked as installed **only in that new instance**.
- **Accounts**: offline usernames.

Data lives in the OS app-data folder (`~/Library/Application Support/dev.p1e2r.Lodestone` on macOS,
`%APPDATA%\p1e2r\Lodestone\data` on Windows, `~/.local/share/lodestone` on Linux).

## Download

Get the latest build from **Releases**:

| Platform | Download |
|----------|----------|
| Windows (64-bit) | `Lodestone-windows-x64-setup.exe` (installer, no admin needed) |
| macOS, Apple Silicon | `Lodestone-macos-arm64.dmg` |
| macOS, Intel | `Lodestone-macos-intel.dmg` |
| Linux (x86_64) | `Lodestone-linux-x64.tar.gz` |

- **Windows:** installs per-user to `%LOCALAPPDATA%\Programs\Lodestone` with a Start Menu entry
  (searchable) and an uninstaller in Settings → Apps.
- **macOS:** drag to Applications. The app isn't notarized; run once:
  `xattr -dr com.apple.quarantine /Applications/Lodestone.app`
- If you run Lodestone from somewhere else (e.g. Downloads), it moves itself to your per-user
  programs folder (`%LOCALAPPDATA%\Programs\Lodestone` or `~/Applications`) and relaunches.

Lodestone checks for new releases on startup (and from **Settings → Updates**); click **Update** in
the sidebar to install and relaunch. Instances, worlds, accounts and settings are kept: they live in
the app-data folder, not next to the app.

## Build

```sh
cargo build --release
```

Linux needs `libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev`.

Command-line mode for testing: `lodestone --cli install 26.2 fabric 0.20.1`.

## Layout

| Path | What |
|------|------|
| `src/core` | Launcher engine: versions, libraries, assets, Java, loaders, Modrinth, servers, accounts |
| `src/ui` | GPUI interface |

## Credits

- Fonts (SIL Open Font License 1.1, licenses in `assets/fonts/`):
  [Silkscreen](https://github.com/googlefonts/silkscreen) (titles),
  [Pixelify Sans](https://github.com/eifetx/Pixelify-Sans) (text),
  [VT323](https://fonts.google.com/specimen/VT323) (console)
- Icons: [Lucide](https://lucide.dev), ISC License (via gpui-kit)

## License

MIT. Not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.
