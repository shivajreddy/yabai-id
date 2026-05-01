# yabai-id

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A macOS menu bar spaces indicator for [yabai](https://github.com/koekeishiya/yabai), written in Rust.

Inspired by [YabaiIndicator](https://github.com/xiamaz/YabaiIndicator).

## What it looks like

Each Mission Control space appears as a rounded button in the menu bar:

- **White fill + black text** — active (focused) space
- **Black fill + white text + white border** — inactive spaces

Clicking a button focuses that space via yabai.

## How it works

Space focus state is read directly from the private `SkyLight.framework` (the macOS window server) via `SLSCopyManagedDisplaySpaces` — the same approach used by YabaiIndicator. This means updates are instant: no polling, no IPC delay, no querying yabai.

`NSWorkspaceActiveSpaceDidChangeNotification` triggers a redraw the moment macOS commits the space switch.

Space focus commands (`space --focus`) are sent to yabai via its Unix socket using the yabai wire protocol directly.

## Requirements

- macOS
- [yabai](https://github.com/koekeishiya/yabai) running
- Rust toolchain (to build from source)

## Build & run

```sh
git clone https://github.com/shivajreddy/yabai-id.git
cd yabai-id
cargo build --release
./target/release/yabai-id
```

Or during development:

```sh
cargo run
```

## Optional: yabai signal integration

`yabai-id` also listens on `/tmp/yabai-id.socket` (and `/tmp/yabai-indicator.socket` for compatibility with existing YabaiIndicator setups) for explicit refresh signals.

Add these to your `.yabairc` if you want event-driven refreshes for things like window changes:

```sh
yabai -m signal --add event=space_changed \
    action='echo "refresh" | nc -U /tmp/yabai-id.socket'
yabai -m signal --add event=display_added \
    action='echo "refresh" | nc -U /tmp/yabai-id.socket'
yabai -m signal --add event=display_removed \
    action='echo "refresh" | nc -U /tmp/yabai-id.socket'
```

Manual refresh:

```sh
echo "refresh" | nc -U /tmp/yabai-id.socket
```

## Right-click menu

- **Toggle display separators** — show/hide `|` divider between displays on multi-monitor setups
- **Refresh** — manual refresh (`r`)
- **Quit yabai-id** (`q`)

## Architecture

| Component | Details |
|---|---|
| Menu bar | `NSStatusItem` with a custom `NSView` subclass |
| Drawing | `NSBezierPath` rounded rects drawn in `drawRect:` |
| Space state | `SkyLight.framework` private API (`SLSCopyManagedDisplaySpaces`) via `dlopen`/`dlsym` |
| Auto-refresh | `NSWorkspaceActiveSpaceDidChangeNotification` |
| Focus command | yabai Unix socket (`/tmp/yabai_$USER.socket`) wire protocol |
| Signal refresh | Unix socket server at `/tmp/yabai-id.socket` |
| Threading | `dispatch2` (`dispatch_async` to main queue) |
| ObjC bindings | `objc2` 0.6 + `objc2-app-kit` 0.3 |
