<!-- LOGO -->
<h1>
<p align="center">
  <img src="https://rioterm.com/assets/rio-logo.png" alt="Rio terminal logo" width="128">
  <br>Rio Terminal
</h1>
  <p align="center">
    Rio is a modern terminal built to run everywhere.
    <br />
    <a href="#about">About</a>
    ·
    <a href="https://rioterm.com/docs/install">Install</a>
    ·
    <a href="https://rioterm.com/docs/config">Config</a>
    ·
    <a href="https://rioterm.com/changelog">Changelog</a>
    ·
    <a href="https://github.com/sponsors/raphamorim">Sponsor</a>
  </p>
</p>

> [!NOTE]
> **This is a fork** of [raphamorim/rio](https://github.com/raphamorim/rio) maintained by [@nikicat](https://github.com/nikicat), carrying a set of Linux-focused fixes and features on top of upstream. See the [summary of changes](#changes-relative-to-upstream) below. Linux-only release builds are published from this fork; for the canonical project, install from upstream.

Documentation: [rioterm.com](https://rioterm.com).

## Changes relative to upstream

This fork adds the following on top of upstream `main`:

| Area | Change | Type |
| --- | --- | --- |
| Bell | Integrate the Linux bell with the desktop environment ([#1616]); per-tab bell indicator ([#1617]); coalesce BEL floods so a binary `cat` can't freeze the app | feat / fix |
| Notifications | Desktop notifications are clickable (Linux/D-Bus) and switch to the tab that rang; `notify` on by default via a new `ActivateRoute` event; `Window::activate_token` for out-of-band activation | feat |
| Splits | `ToggleSplitZoom` action to maximize the focused split, with a zoom indicator | feat |
| Selection | Smart-selection rules on double-click with an `[smart-selection]` config table (hot-reload); OSC 8 fast path for double-click; claim the Wayland PRIMARY selection on mouse release; start a fresh selection when a click switches panes | feat / fix |
| Fonts | Configurable per-slot font weight for bundled Cascadia Code; resolve the `wght` axis at rasterize time (correct variable-weight rendering); trigger font-fallback discovery on Linux/Windows shape paths | feat / fix |
| Tab titles | Render emoji/CJK/symbols in tab titles via per-codepoint fontconfig fallback | fix |
| Hidden tabs | Route backend events to hidden-tab panels (background tabs stay live) | fix |
| Quit dialog | Always present the quit-confirmation dialog, dispatch `RioEvent::Quit` so the Quit action works, and repaint on dismiss | fix |
| Layout | Refresh the Taffy root size when the scaled margin changes; align URL-hint hit-box with the visible glyph past wide chars | fix |
| Build / CI | Linux-only releases via OSS GoReleaser; fail clippy on warnings (`-D warnings`); Nix `rust-overlay` updated for Rust 1.96; `sctk-adwaita` pinned to a fork carrying CSD title-font fallback | chore |

[#1616]: https://github.com/raphamorim/rio/issues/1616
[#1617]: https://github.com/raphamorim/rio/issues/1617

## Supporting the Project

If you use and like Rio, please consider sponsoring it: your support helps to cover the fees required to maintain the project and to validate the time spent working on it!

[![Sponsor Rio terminal](https://img.shields.io/github/sponsors/raphamorim?label=Sponsor%20Rio&logo=github&style=for-the-badge)](https://github.com/sponsors/raphamorim)

## Packaging

[![Packaging status](https://repology.org/badge/vertical-allrepos/rio-terminal.svg?columns=3)](https://repology.org/project/rio-terminal/versions)

> Demo with split and CRT on MacOS

![Demo Rio 0.2.0 on MacOS](https://rioterm.com/assets/posts/0.2.0/demo-rio.png)

> Demo with blurred background on Linux

![Demo blurred background](https://rioterm.com/assets/demos/demos-nixos-blur.png)

> Demo of Rio running on a Steam Deck

![Demo of Rio running on a Steam Deck](https://rioterm.com/assets/demos/demo-flatpak-steamdeck.jpg)

## Minimal stable rust version

Rio's MSRV is 1.96.0.
