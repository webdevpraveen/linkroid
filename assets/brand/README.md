# The Linkroid mark

The Linkroid Mark: two connecting loops, representing the phone and the PC, around a center node, symbolizing real-time cross-device continuity.

| File | What it is |
|---|---|
| `icon.png` | The full-color app icon: obsidian slate tile, electric cyan glow, accent rim |
| `icon-flat.svg` | The same without the glow and highlights, for flat contexts |
| `icon-mono.svg` | The mark alone in white, for one-color uses (notifications, themed icons) |
| `icon-512.png` | `icon.png` at 512 × 512 |

The apps draw the mark themselves from the same geometry:
`desktop/app/src/mark.rs` (the Windows icon, tray and notifications at every
size) and `android/app/src/main/res/drawable/ic_launcher_*.xml` and
`ic_notification.xml`.

Colors (Brand accent palette): tile `#0B132B`, glow `#0284C7`, phone loop `#00E676`, PC loop `#0091FF`, center nexus `#38BDF8`.

The name and the mark are covered by the [trademark policy](../../TRADEMARKS.md).
