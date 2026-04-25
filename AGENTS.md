# OPPi Agent Instructions

If `.oppi-plans/INDEX.md` exists, read it before implementing non-trivial changes.

Core rules:

- OPPi is based on Pi as an upstream dependency, not a fork.
- Prefer Pi extensions, skills, prompts, themes, and SDK APIs before custom runtime code.
- Keep behavior modules reusable from both stock Pi and the future `oppi` CLI.
- Do not inspect, clone, or copy leaked proprietary source. Use public behavior descriptions, user-provided summaries, and open-source references instead.
- Open-source references that are allowed for source-level study: Pi, Oh My Pi, OpenAI Codex CLI, and Claude-Mem subject to their licenses.
- Avoid throwaway implementations for native-heavy features. If Rust/native is the right final form, document and defer instead of building a temporary duplicate.
- Keep public docs free of private planning notes. Local planning lives in `.oppi-plans/`, which is gitignored.
- Favor concise, polished UX: professional, sharp, a little playful, but never noisy.
