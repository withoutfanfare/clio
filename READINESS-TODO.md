# Clio — Public-Readiness TODO

> Companion to [`../Tauri Apps - Public Readiness Plan.md`](../Tauri%20Apps%20-%20Public%20Readiness%20Plan.md).
> Every item below is transcribed from that report — see **Source coverage** at the bottom to confirm nothing was dropped.

| | |
|---|---|
| Path (git root) | `clio/` |
| App location | `crates/clio-tauri` |
| Remote | `withoutfanfare/clio` |
| Visibility | **PUBLIC** |
| Bundle ID | `com.clio.desktop` |
| Overall risk (report §10) | **Low** |
| CSP | sound (`default-src 'self'; script-src 'self'`, no `unsafe-eval`) |

## How to use
`- [ ]` open · `- [x]` done · `- [~]` blocked / decision needed (note why). Phases mirror report §9. **P0 → P1 are gating.**

## §4 "Safely public" scorecard (transcribed from report §10)

| # | Criterion | Status | Note |
|---|---|---|---|
| 1 | No secrets / PII in git history | ✅ | clean (§7) |
| 2 | No secrets / PII in working tree | ✅ | OpenAI key from settings/env, never hardcoded (§7) |
| 3 | LICENSE present | ❌ | none (§6.1) |
| 4 | Builds from clean clone | ❌ | `ui/.npmrc` → Verdaccio (§6.2) |
| 5 | CSP not null | ✅ | sound (§7) |
| 6 | Least-privilege capabilities | ✅ | minimal (§7) |
| 7 | No dangerous code paths | ✅ | `osascript`/`pbcopy` via temp file/stdin (§7) |
| 8 | No undisclosed telemetry | ✅ | no telemetry (§1) |
| 9 | No confidential client data | ✅ | (§10) |
| 10 | README adequate | ✅ | (§10) |
| 11 | Secret-scanning in CI | ❌ | none (§6.7) |

## P0 — Incident response
_None for Clio_ (history and working tree clean).

## P1 — Blockers before publicising (gating)
- [ ] **Add LICENSE** to repo root — decision §8.1 (default MIT). Set `license`/`author` in `package.json`. (report §6.1, §9 P1.1)
- [ ] **Solve `@stuntrocket/ui` distribution** — `ui/.npmrc` → Verdaccio; pending §8.2 decision (default: publish to public npm). (report §6.2, §9 P1.2)
- [ ] **Fix stale URL** — `CHANGELOG.md` links `github.com/dannyharding/clio` (should be `withoutfanfare/clio`). (report §7 clio)

## P2 — Security hardening
- [ ] **Secret-scanning CI** — `gitleaks` pre-commit + GitHub Actions (report Appendix C). (report §6.7, §9 P2.4)
- [ ] **Standardise `.gitignore`** to report Appendix D. (report §6.7, §9 P2.6)
- [ ] **Wire `npm audit` / `cargo audit` into CI** with fail-on-high gate (currently 0 high). (report §9 P2.7)

## P3 — Polish & privacy presentation
- [ ] **Bundle-ID decision** — `com.clio.desktop` → unified scheme (default `co.stuntrocket.clio`). ⚠️ decide **before** notarisation. (report §6.5, §8.3, §9 P3.1)
- [ ] **Scrub `/Users/dannyharding/.clio/shared.db`** from example JSON in docs. (report §6.6, §7 clio)
- [ ] **Add privacy statement** to README (report Appendix F). (report §9 P3.5)
- [ ] **Add `SECURITY.md`** (report Appendix G). (report §9 P3.6)

## Source coverage
Maps **every Clio mention in the main report** to a row above (all copied ✅).

| Report ref | What it says about Clio | Landed in | Copied |
|---|---|---|---|
| §2 table | path/app at `crates/clio-tauri`/remote/visibility/bundle id | header | ✅ |
| §6.1 | no LICENSE | P1 | ✅ |
| §6.2 | `ui/.npmrc` → Verdaccio (no token) — unbuildable for cloners | P1 | ✅ |
| §6.5 | bundle id `com.clio.desktop` | P3 | ✅ |
| §6.6 | `/Users/dannyharding/.clio/shared.db` in docs example | P3 | ✅ |
| §6.7 | no secret-scanning CI | P2 | ✅ |
| §7 Clio | history/tree clean; stale CHANGELOG URL; osascript/pbcopy safe; npm audit 0 | scorecard + P1/P2 | ✅ |
| §8.1/§8.2/§8.3 | licence / UI / bundle-id decisions | P1/P3 | ✅ |
| §9 P1.1/§9 P1.2 | LICENSE, UI distribution | P1 | ✅ |
| §9 P2.4/P2.6/P2.7 | gitleaks, gitignore, audit CI | P2 | ✅ |
| §9 P3.1/P3.5/P3.6 | bundle id, privacy, SECURITY.md | P3 | ✅ |
| §10 row | full scorecard | scorecard | ✅ |
