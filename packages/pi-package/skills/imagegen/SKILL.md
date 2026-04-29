---
name: imagegen
description: Generate or edit images with Pi's image_gen tool, including Codex native image_generation and OpenAI GPT Image API fallback. Use when the user asks to create, generate, draw, render, or edit images.
---

# Image Generation

Use this skill when the user asks to create, generate, draw, render, or edit an image.

## Default behavior

- Prefer the `image_gen` tool for normal image generation and editing requests.
- If the user asks you to pick a subject yourself, choose a concrete subject and call `image_gen`; do not ask a follow-up unless key requirements are missing.
- Write a rich prompt: subject, scene, style/medium, composition, lighting, mood, palette, materials/textures, text to include verbatim, constraints, and things to avoid when relevant.
- The tool saves generated images to the workspace and returns image content for preview/follow-up.

## Backend routing

- Use `image_gen` with no explicit backend/model for ordinary requests. It dynamically prefers Codex native `image_generation` when available and falls back to the OpenAI Images API when configured.
- Use `backend=image_api` only when the user explicitly asks for API/model controls, multiple outputs (`n > 1`), masks, or exact GPT Image parameters.
- The Image API fallback defaults to `gpt-image-2`, `size=auto`, `quality=medium`, and `outputFormat=png`.

## GPT Image 2 guidance

- `gpt-image-2` supports `quality` values `low`, `medium`, `high`, and `auto`.
- `gpt-image-2` sizes may be `auto` or `WIDTHxHEIGHT` if all constraints hold: max edge `<= 3840px`, both edges multiples of `16px`, long-to-short ratio `<= 3:1`, total pixels between `655,360` and `8,294,400`.
- Good near-4K examples: `3840x2160`, `2160x3840`, `2560x1440`, `1440x2560`, `2048x2048`.
- Do not set `inputFidelity` with `gpt-image-2`; image inputs are always high fidelity.
- `gpt-image-2` does not support true `background=transparent`.

## Transparency

- Do not silently downgrade from `gpt-image-2` to `gpt-image-1.5`.
- If the user requests true/native transparency, explain that `gpt-image-2` does not support transparent backgrounds and ask before using `backend=image_api`, `model=gpt-image-1.5`, `background=transparent`, and `outputFormat=png` or `webp`.
- If the user already explicitly requested `gpt-image-1.5` or true API fallback transparency, proceed.

## Editing

- For source-image edits, pass image file paths in `images` and describe the transformation clearly in `prompt`.
- For mask-based edits, use `backend=image_api` with `mask`; masks must be PNG files with transparent areas indicating where to edit.
