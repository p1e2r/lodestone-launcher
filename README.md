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
- A unique pixel-art scene for every instance

## Using it

- **Library / Servers**: create instances or servers for any version and loader, or import a `.mrpack`.
- **Instance page**: Play, manage mods / resource packs / shaders, view the game log, set memory.
- **Server page**: Start/Stop, live console with commands, join address, port / MOTD / online mode / EULA.
- **Discover**: search Modrinth; pick an instance to see only compatible content.
- **Accounts**: offline usernames.

Data lives in the OS app-data folder (`~/Library/Application Support/dev.p1e2r.Lodestone` on macOS,
`%APPDATA%\p1e2r\Lodestone\data` on Windows, `~/.local/share/lodestone` on Linux).

## Download

Builds for macOS (Apple Silicon), Windows and Linux are produced by GitHub Actions on every push
(see the **Actions** tab). The macOS app isn't notarized; after unzipping run:

```sh
xattr -dr com.apple.quarantine Lodestone.app
```

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

## License

MIT. Not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.
