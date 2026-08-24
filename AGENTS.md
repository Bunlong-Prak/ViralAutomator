# AGENTS

## Overview
This project uses Vue 3 with TypeScript for the frontend and Rust for the backend.
Vue code lives in `src/` and `src/components`, while the Rust Tauri backend lives in `src-tauri/`.

## Testing
Before committing changes, run the following commands:

### JavaScript/TypeScript
- `npm run lint:fix` to apply ESLint fixes and formatting.
- `npm run test:unit` to run unit tests.
- `npm run test:e2e` to run end-to-end tests.
- `npm run build` to ensure the app builds.

To run e2e tests locally, install Playwright browsers and required system packages:

- `npx playwright install --with-deps`

### Rust (inside `src-tauri/`)
- `cargo fmt --all` to format Rust code.
- `cargo clippy --all-targets -- -D warnings` to lint.
- `cargo test` to run backend tests.

Rust linting and tests may fail in this environment because the `glib-2.0` system library is missing.

## Searching
Use `rg` for searching the codebase; avoid recursive `grep` or `ls` commands.

## Commit style
Write commit messages in the form `type: summary`, e.g. `fix: update progress bar logic`.


<claude-mem-context>
# Memory Context

# [ViralAutomator] recent context, 2026-08-23 10:27pm CDT

Legend: 🎯session 🔴bugfix 🟣feature 🔄refactor ✅change 🔵discovery ⚖️decision 🚨security_alert 🔐security_note
Format: ID TIME TYPE TITLE
Fetch details: get_observations([IDs]) | Search: mem-search skill

Stats: 15 obs (7,202t read) | 307,048t work | 98% savings

### Aug 23, 2026
170 10:11p 🔵 ViralAutomator Project Identified — Vue.js Composables Pattern
171 " 🔵 ViralAutomator = Open Video Downloader (youtube-dl-gui) v3.2.1
172 " 🔵 OVD Rust Backend Architecture: AI Module, Scheduling Pipelines, Stronghold, Sentry
173 " 🔵 OVD Frontend Architecture: Pinia Media Store, State Machine, Router Structure
174 10:12p 🔵 "ViralAutomator" Feature Layer: Full AI Post-Processing Pipeline Built into OVD Fork
175 " 🔵 Binary Manager: Signed Manifest System for yt-dlp/FFmpeg Auto-Install
176 10:15p 🔵 ViralAutomator Project Structure
177 " 🔵 ViralAutomator is Open Video Downloader v3.2.1
178 10:16p 🔵 AI Settings and Uniqueness/Viral Features in Config
179 " 🔵 Binary Install Flow and Route Structure
180 " 🔵 Complete AI Post-Processing Pipeline in Rust Backend
181 " 🔵 FFmpeg filter_complex Pipeline for Video Uniqueness
183 " 🔵 Uncommitted Changes Across AI Pipeline, UI, and Locale Files
184 " 🟣 OutputSettingsEditor Gained Full Uniqueness Tab UI
182 10:17p 🔵 YouTube Shorts Auto-Poster via Playwright Browser Automation

Access 307k tokens of past work via get_observations([IDs]) or mem-search skill.
</claude-mem-context>