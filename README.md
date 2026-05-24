# Cadenza

Cadenza is a desktop app for CharaChorder chording-device users. It globally logs
keystrokes, detects whether text was typed manually ("words") or fired as a chord
(by inter-character timing), and surfaces analytics: WPM, chord suggestions, and
chord proficiency.

## License

Cadenza is licensed under the **GNU Affero General Public License v3.0 (AGPL-3.0)**.
See [`LICENSE`](./LICENSE) for the full text.

This project derives logic from two CharaChorder projects, both AGPL-3.0:

- **`nexus`** — Freqlog keystroke detection / classification.
- **`DeviceManager`** — serial device communication and chord map handling.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
