# ui — Vue 3 desktop frontend

Local contract for the desktop UI. Inherits repo-wide rules from the root
CLAUDE.md. This doc owns the frontend stack and its build/typecheck workflow.
This is the only JavaScript/TypeScript boundary in the repo — a different
toolchain (npm/Vite) from the Rust workspace.

## Purpose

The Vue 3 desktop interface, rendered inside the `clio-tauri` shell. Talks to the
Tauri backend commands; holds no business logic of its own (that lives in
`clio-core`).

## Structure (`src/`)

| Path | Holds |
|------|-------|
| `api/` | Typed wrappers over Tauri commands (`memory.ts`, `types.ts`). |
| `stores/` | Pinia state (`memories.ts`). |
| `views/` | Routed pages (Home, Namespaces, Stats, Tools, ContextBuilder). |
| `components/` | Reusable components (list, drawer, command palette, etc.). |
| `composables/` | Reactive helpers (`useKeyboard`, `useAutoSave`, …). |
| `router/`, `utils/` | Routing and pure helpers. |

## Stack

Vue 3 + Pinia + Vue Router, built with Vite; TypeScript checked by `vue-tsc`.
Shared design system via `@stuntrocket/ui`. Tauri APIs via `@tauri-apps/api`.

## Work Guidance

- British English in all user-facing text.
- Keep components presentational; route data access through `api/` and `stores/`.
- Reuse `@stuntrocket/ui` primitives before adding bespoke components.

## Verification

- Install deps: `npm install` (run in `ui/`).
- Typecheck + build: `npm run build` (`vue-tsc --noEmit` then `vite build`).
- Dev with hot reload (full app): `./dev.sh` from the repo root.
