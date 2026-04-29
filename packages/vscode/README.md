# @oppiai/vscode

Future VS Code extension.

The extension should be a client of OPPi server/runtime, not the core runtime itself.

Target behavior:

- one workspace/window talks to its own OPPi session/server context
- React webview for chat, approvals, todos, memory, plugin browser, and images
- terminal integration remains available
- multi-agent tabs are primarily a VS Code + future TUI feature
