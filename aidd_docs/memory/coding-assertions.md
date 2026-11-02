# Coding Assertions

Les contrôles qui doivent passer pour qu'un code compte comme terminé. Minimaux, lancés après chaque changement.

## Before commit

Le contrôle rapide (fonctionne sous Linux/WSL, `source ~/.cargo/env` si `cargo` est introuvable).

| Order | Command | Checks |
| ----- | ------- | ------ |
| 1 | `cargo fmt --all -- --check` | format |
| 2 | `cargo clippy -p gsf-core -p gsf-sync --all-targets -- -D warnings` | lint, zéro avertissement |
| 3 | `cargo test -p gsf-core -p gsf-sync` | tests unitaires et d'intégration |

## Before push

Le contrôle lourd, quand `src-tauri` ou `ui` change.

| Order | Command | Checks |
| ----- | ------- | ------ |
| 1 | `cargo clippy -p gsf-bridge --target x86_64-pc-windows-msvc --all-targets -- -D warnings`, avec un faux `llvm-rc` et un faux `lib.exe` dans le `PATH` (voir `desktop.md`) | type-check Windows de l'application |
| 2 | CI `windows-build` verte sur la PR | build NSIS réel |

## Behavior

I fix is needed, spawn 1 agent per assertion to fix (e.g typechecking / tests / rules violated on category UI = 3 agents).
