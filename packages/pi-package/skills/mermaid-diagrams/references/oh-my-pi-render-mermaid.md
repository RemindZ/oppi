# Oh My Pi Mermaid reference

Attribution: [Oh My Pi](https://github.com/can1357/oh-my-pi), MIT license.

Reference files inspected from the local allowed clone:

- `.reference/oh-my-pi/packages/coding-agent/src/tools/render-mermaid.ts`
- `.reference/oh-my-pi/packages/coding-agent/src/prompts/tools/render-mermaid.md`
- `.reference/oh-my-pi/packages/coding-agent/src/modes/theme/mermaid-cache.ts`
- `.reference/oh-my-pi/packages/tui/src/components/markdown.ts`

Distilled behavior:

- Tool name: `render_mermaid`.
- Input: Mermaid graph source plus optional render config.
- Output: ASCII diagram text, with optional artifact storage.
- TUI markdown rendering can resolve fenced `mermaid` code blocks into ASCII when a Mermaid renderer is available.
- Invalid Mermaid falls back to normal fenced code rather than breaking display.

OPPi Stage 1 integration:

- OPPi includes a `mermaid-diagrams` skill so the model knows when and how to produce small Mermaid diagrams.
- OPPi registers a `render_mermaid` tool that uses the `beautiful-mermaid` dependency for terminal ASCII previews.
- The tool has a small local fallback for basic `flowchart`/`graph` and `sequenceDiagram` input so missing renderer dependencies fail softly.
- Automatic fenced-block replacement remains later work because it needs deeper Pi renderer hooks or an OPPi-owned TUI layer.
