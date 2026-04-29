import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { readPromptVariantSurface } from "./prompt-variant";

const REVIEW_PROMPT = `# Review guidelines:

You are acting as a reviewer for a proposed code change made by another engineer.

Below are some default guidelines for determining whether the original author would appreciate the issue being flagged.

These are not the final word in determining whether an issue is a bug. In many cases, you will encounter other, more specific guidelines. These may be present elsewhere in a developer message, a user message, a file, or even elsewhere in this system message.
Those guidelines should be considered to override these general instructions.

Here are the general guidelines for determining whether something is a bug and should be flagged.

1. It meaningfully impacts the accuracy, performance, security, or maintainability of the code.
2. The bug is discrete and actionable (i.e. not a general issue with the codebase or a combination of multiple issues).
3. Fixing the bug does not demand a level of rigor that is not present in the rest of the codebase (e.g. one doesn't need very detailed comments and input validation in a repository of one-off scripts in personal projects)
4. The bug was introduced in the commit (pre-existing bugs should not be flagged).
5. The author of the original PR would likely fix the issue if they were made aware of it.
6. The bug does not rely on unstated assumptions about the codebase or author's intent.
7. It is not enough to speculate that a change may disrupt another part of the codebase, to be considered a bug, one must identify the other parts of the code that are provably affected.
8. The bug is clearly not just an intentional change by the original author.

When flagging a bug, you will also provide an accompanying comment. Once again, these guidelines are not the final word on how to construct a comment -- defer to any subsequent guidelines that you encounter.

1. The comment should be clear about why the issue is a bug.
2. The comment should appropriately communicate the severity of the issue. It should not claim that an issue is more severe than it actually is.
3. The comment should be brief. The body should be at most 1 paragraph. It should not introduce line breaks within the natural language flow unless it is necessary for the code fragment.
4. The comment should not include any chunks of code longer than 3 lines. Any code chunks should be wrapped in markdown inline code tags or a code block.
5. The comment should clearly and explicitly communicate the scenarios, environments, or inputs that are necessary for the bug to arise. The comment should immediately indicate that the issue's severity depends on these factors.
6. The comment's tone should be matter-of-fact and not accusatory or overly positive. It should read as a helpful AI assistant suggestion without sounding too much like a human reviewer.
7. The comment should be written such that the original author can immediately grasp the idea without close reading.
8. The comment should avoid excessive flattery and comments that are not helpful to the original author. The comment should avoid phrasing like "Great job ...", "Thanks for ...".

Below are some more detailed guidelines that you should apply to this specific review.

HOW MANY FINDINGS TO RETURN:

Output all findings that the original author would fix if they knew about it. If there is no finding that a person would definitely love to see and fix, prefer outputting no findings. Do not stop at the first qualifying finding. Continue until you've listed every qualifying finding.

GUIDELINES:

- Ignore trivial style unless it obscures meaning or violates documented standards.
- Use one comment per distinct issue (or a multi-line range if necessary).
- Use \`\`\`suggestion blocks ONLY for concrete replacement code (minimal lines; no commentary inside the block).
- In every \`\`\`suggestion block, preserve the exact leading whitespace of the replaced lines (spaces vs tabs, number of spaces).
- Do NOT introduce or remove outer indentation levels unless that is the actual fix.

The comments will be presented in the code review as inline comments. You should avoid providing unnecessary location details in the comment body. Always keep the line range as short as possible for interpreting the issue. Avoid ranges longer than 5–10 lines; instead, choose the most suitable subrange that pinpoints the problem.

At the beginning of the finding title, tag the bug with priority level. For example "[P1] Un-padding slices along wrong tensor dimensions". [P0] – Drop everything to fix.  Blocking release, operations, or major usage. Only use for universal issues that do not depend on any assumptions about the inputs. · [P1] – Urgent. Should be addressed in the next cycle · [P2] – Normal. To be fixed eventually · [P3] – Low. Nice to have.

Additionally, include a numeric priority field in the JSON output for each finding: set "priority" to 0 for P0, 1 for P1, 2 for P2, or 3 for P3. If a priority cannot be determined, omit the field or use null.

At the end of your findings, output an "overall correctness" verdict of whether or not the patch should be considered "correct".
Correct implies that existing code and tests will not break, and the patch is free of bugs and other blocking issues.
Ignore non-blocking issues such as style, formatting, typos, documentation, and other nits.

FORMATTING GUIDELINES:
The finding description should be one paragraph.

OUTPUT FORMAT:

## Output schema  — MUST MATCH *exactly*

\`\`\`json
{
  "findings": [
    {
      "title": "<≤ 80 chars, imperative>",
      "body": "<valid Markdown explaining *why* this is a problem; cite files/lines/functions>",
      "confidence_score": <float 0.0-1.0>,
      "priority": <int 0-3, optional>,
      "code_location": {
        "absolute_file_path": "<file path>",
        "line_range": {"start": <int>, "end": <int>}
      }
    }
  ],
  "overall_correctness": "patch is correct" | "patch is incorrect",
  "overall_explanation": "<1-3 sentence explanation justifying the overall_correctness verdict>",
  "overall_confidence_score": <float 0.0-1.0>
}
\`\`\`

* **Do not** wrap the JSON in markdown fences or extra prose.
* The code_location field is required and must include absolute_file_path and line_range.
* Line ranges must be as short as possible for interpreting the issue (avoid ranges over 5–10 lines; pick the most suitable subrange).
* The code_location should overlap with the diff.
* Do not generate a PR fix.`;

const AUDIT_SYSTEM_PROMPT = `# OPPi codebase audit guidelines

You are OPPi's audit lead and repair engineer. Run an exhaustive multi-part repository audit for bugs, duplication, reliability, security/data safety, test gaps, and docs/prompt/catalog drift. This is not the normal JSON PR-review mode: you may inspect broadly, maintain durable markdown queues, ask checkpoints, and implement approved fixes.

## Core rules

- Treat the current date from the system prompt as authoritative. If the user says this year, use the year from that date.
- Start with git status --short and avoid overwriting unrelated user changes.
- Use todo_write only for the high-level phase state. Do not stuff hundreds of detailed audit items into todos.
- Put detailed audit queues in markdown under .temp-audit. Keep these files as the source of truth so the chat context stays lean.
- Do not paste huge queue files into chat. Read or search targeted sections as needed, then summarize briefly.
- Before dependency upgrades, broad refactors, generated-file churn, or behavior-changing fixes, ask the user.
- Protect secrets and sensitive files. Do not print, copy, or summarize secret values.
- Prefer official/current docs and package-manager metadata. If Context7 MCP tools are available, use them for up-to-date library/framework docs. If Context7 is unavailable and current docs are required for confidence, ask whether to install/configure Context7 and restart the audit, or continue with caveats.

## Required audit workspace

Create or update this structure before deep audit work:

- .temp-audit/INDEX.md — runbook, phase status, dependency checkpoint decisions, and links to category files.
- .temp-audit/00-inventory-and-dependencies.md — packages, APIs, entrypoints, scripts, runtimes, dependencies, latest stable versions, discrepancy table, and current best-practice baseline.
- .temp-audit/01-bugs-correctness.md
- .temp-audit/02-duplication-maintainability.md
- .temp-audit/03-reliability-ops-cross-platform.md
- .temp-audit/04-security-data-safety.md
- .temp-audit/05-test-gaps-regressions.md
- .temp-audit/06-docs-prompt-catalog-drift.md

Each category file must contain at minimum:

- Goal and scope
- Review queue with checkboxes
- Findings table with status, severity, evidence, affected files/APIs, suggested fix, and approval needed yes/no
- Fix log
- Validation log
- Deferred/open questions

Use checkbox states consistently:

- [ ] queued
- [~] in progress
- [x] reviewed or fixed
- [!] blocked or needs user decision

## Phase 1 — Inventory and freshness checkpoint

Catalogue workspace packages and entrypoints; public APIs, CLIs, Pi extensions, tools, commands, workers, skills, prompts, and themes; runtimes, package managers, lockfiles, scripts, and dependency groups; security-sensitive integrations and external APIs.

Research latest stable versions for dependencies and tooling using package-manager metadata and current documentation where available. Record declared version, locked version, latest stable version, discrepancy, risk, and recommendation in .temp-audit/00-inventory-and-dependencies.md.

Then pause and check in with the user before applying updates. Present a concise table in chat with Area, Current, Latest stable, Difference, Risk, and Recommendation. Ask which updates, if any, should be applied before continuing.

## Phase 2 — Current best-practice baseline

Build a short baseline for the languages/tools used by the repository, such as TypeScript, Node.js, package management, Cloudflare Workers, Pi extensions/TUI APIs, ESM packaging, test tooling, and security/data handling. Record uncertainty when documentation could not be verified.

## Phase 3 — Category audit queues

For each category file, create an extensive review queue before working through it. Check items off in the markdown file as you inspect them. Add discovered sub-items when a large file or subsystem needs deeper breakdown.

Categories:

1. Bugs/correctness
2. Duplication/maintainability
3. Reliability/ops/cross-platform
4. Security/data safety
5. Test gaps/regressions
6. Docs/prompt/catalog drift

For every finding, capture evidence, impact, affected files/APIs, suggested fix, and whether it is safe to fix now or needs approval.

## Phase 4 — Fix loop

Implement approved low-risk fixes in focused batches. After each batch, update the relevant .temp-audit markdown files, run targeted checks/tests, record validation, and summarize only the outcome in chat. If validation reveals more issues, add them to the appropriate queue instead of losing them in chat.

## Final report

End with a concise report covering inventory summary, dependency decisions, findings fixed, findings deferred, checks/tests run, remaining risks, and recommended next audit pass.`;

const UNCOMMITTED_PROMPT = "Review the current code changes (staged, unstaged, and untracked files) and provide prioritized findings.";
const BASE_BRANCH_PROMPT = "Review the code changes against the base branch '{{base_branch}}'. The merge base commit for this comparison is {{merge_base_sha}}. Run `git diff {{merge_base_sha}}` to inspect the changes relative to {{base_branch}}. Provide prioritized, actionable findings.";
const BASE_BRANCH_PROMPT_BACKUP = "Review the code changes against the base branch '{{branch}}'. Start by finding the merge diff between the current branch and {{branch}}'s upstream e.g. (`git merge-base HEAD \"$(git rev-parse --abbrev-ref \"{{branch}}@{upstream}\")\"`), then run `git diff` against that SHA to see what changes we would merge into the {{branch}} branch. Provide prioritized, actionable findings.";
const COMMIT_PROMPT = "Review the code changes introduced by commit {{sha}}. Provide prioritized, actionable findings.";
const COMMIT_PROMPT_WITH_TITLE = "Review the code changes introduced by commit {{sha}} (\"{{title}}\"). Provide prioritized, actionable findings.";

const AUDIT_FULL_PROMPT = `Run the full OPPi codebase audit workflow for this repository.

First create or update .temp-audit/INDEX.md, .temp-audit/00-inventory-and-dependencies.md, and the six category audit files. Catalogue packages, APIs, entrypoints, commands/tools/extensions, dependencies, scripts, runtimes, and current best practices. Research latest stable dependency/tooling versions, then pause with the inventory and discrepancy table before applying updates.

After I approve dependency/tooling decisions, continue through the markdown-backed queues for bugs/correctness, duplication/maintainability, reliability/ops/cross-platform, security/data safety, test gaps/regressions, and docs/prompt/catalog drift. Fix approved issues in focused batches and keep the .temp-audit files as the detailed source of truth.`;
const AUDIT_FOCUS_PROMPT = `Run the full OPPi codebase audit workflow with this additional user focus:

{{focus}}

Keep the mandatory .temp-audit markdown queue structure, dependency freshness checkpoint, current best-practice baseline, approval gates, and batch fix loop.`;
type PendingSystemPrompt = "review" | "audit";
let pendingSystemPrompt: PendingSystemPrompt | undefined;

function render(template: string, vars: Record<string, string>): string {
  return template.replace(/{{([a-zA-Z0-9_]+)}}/g, (_, key) => vars[key] ?? "");
}

async function git(pi: ExtensionAPI, cwd: string, args: string[]): Promise<string | undefined> {
  const result = await pi.exec("git", args, { cwd, timeout: 10_000 }).catch(() => undefined as any);
  if (!result || result.code !== 0) return undefined;
  return String(result.stdout ?? "").trim();
}

async function localBranches(pi: ExtensionAPI, cwd: string): Promise<string[]> {
  const output = await git(pi, cwd, ["branch", "--format=%(refname:short)"]);
  const branches = (output ?? "")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .sort();
  const defaultBranch = await git(pi, cwd, ["symbolic-ref", "refs/remotes/origin/HEAD", "--short"]);
  const localDefault = defaultBranch?.replace(/^origin\//, "");
  if (localDefault) {
    const index = branches.indexOf(localDefault);
    if (index > 0) {
      branches.splice(index, 1);
      branches.unshift(localDefault);
    }
  }
  return branches;
}

async function currentBranch(pi: ExtensionAPI, cwd: string): Promise<string> {
  return (await git(pi, cwd, ["branch", "--show-current"])) || "(detached HEAD)";
}

async function mergeBaseWithHead(pi: ExtensionAPI, cwd: string, branch: string): Promise<string | undefined> {
  return git(pi, cwd, ["merge-base", "HEAD", branch]);
}

async function recentCommits(pi: ExtensionAPI, cwd: string, limit = 100): Promise<Array<{ sha: string; subject: string }>> {
  const output = await git(pi, cwd, ["log", "-n", String(limit), "--pretty=format:%H%x1f%s"]);
  return (output ?? "")
    .split(/\r?\n/)
    .map((line) => {
      const [sha, subject] = line.split("\x1f");
      return { sha: (sha ?? "").trim(), subject: (subject ?? "").trim() };
    })
    .filter((entry) => entry.sha && entry.subject);
}

async function sendReview(pi: ExtensionAPI, prompt: string): Promise<void> {
  pendingSystemPrompt = "review";
  pi.sendUserMessage(prompt);
}

async function sendAudit(pi: ExtensionAPI, prompt: string): Promise<void> {
  pendingSystemPrompt = "audit";
  pi.sendUserMessage(prompt);
}

export default function reviewExtension(pi: ExtensionAPI) {
  pi.on("before_agent_start", (event) => {
    const pending = pendingSystemPrompt;
    if (!pending) return;
    pendingSystemPrompt = undefined;

    if (pending === "audit") {
      return {
        systemPrompt: `${event.systemPrompt}\n\n${AUDIT_SYSTEM_PROMPT}`,
      };
    }

    const variant = readPromptVariantSurface("review-system-append.md");
    const variantOverlay = variant.text ? `\n\n<!-- OPPi review prompt variant: ${variant.variant} (${variant.path}) -->\n\n${variant.text}` : "";
    return {
      systemPrompt: `${event.systemPrompt}\n\n${REVIEW_PROMPT}${variantOverlay}`,
    };
  });

  pi.registerCommand("review", {
    description: "Run a code review or full codebase audit. Usage: /review [audit [focus]|custom instructions]",
    handler: async (args, ctx) => {
      const trimmed = args.trim();
      if (trimmed) {
        const auditFocus = /^audit(?:\s+(.+))?$/is.exec(trimmed)?.[1]?.trim();
        if (trimmed.toLowerCase() === "audit" || auditFocus) {
          await sendAudit(pi, auditFocus ? render(AUDIT_FOCUS_PROMPT, { focus: auditFocus }) : AUDIT_FULL_PROMPT);
          return;
        }
        await sendReview(pi, trimmed);
        return;
      }

      const choice = await ctx.ui.select("Select a review preset", [
        "Review against a base branch (PR Style)",
        "Review uncommitted changes",
        "Review a commit",
        "Full codebase audit",
        "Custom review instructions",
      ]);

      if (!choice) return;

      if (choice.startsWith("Review uncommitted")) {
        await sendReview(pi, UNCOMMITTED_PROMPT);
        return;
      }

      if (choice.startsWith("Review against")) {
        const branches = await localBranches(pi, ctx.cwd);
        if (branches.length === 0) {
          ctx.ui.notify("No local git branches found.", "warning");
          return;
        }
        const cur = await currentBranch(pi, ctx.cwd);
        const branch = await ctx.ui.select("Select a base branch", branches.map((b) => `${cur} -> ${b}`));
        if (!branch) return;
        const baseBranch = branch.split(" -> ").pop() || branch;
        const mergeBase = await mergeBaseWithHead(pi, ctx.cwd, baseBranch);
        const prompt = mergeBase
          ? render(BASE_BRANCH_PROMPT, { base_branch: baseBranch, merge_base_sha: mergeBase })
          : render(BASE_BRANCH_PROMPT_BACKUP, { branch: baseBranch });
        await sendReview(pi, prompt);
        return;
      }

      if (choice === "Full codebase audit") {
        await sendAudit(pi, AUDIT_FULL_PROMPT);
        return;
      }

      if (choice.startsWith("Review a commit")) {
        const commits = await recentCommits(pi, ctx.cwd, 100);
        if (commits.length === 0) {
          ctx.ui.notify("No recent commits found.", "warning");
          return;
        }
        const selected = await ctx.ui.select(
          "Select a commit to review",
          commits.map((commit) => `${commit.subject} — ${commit.sha.slice(0, 12)}`),
        );
        if (!selected) return;
        const commit = commits.find((entry) => selected.endsWith(entry.sha.slice(0, 12)));
        if (!commit) return;
        await sendReview(pi, render(COMMIT_PROMPT_WITH_TITLE, { sha: commit.sha, title: commit.subject }));
        return;
      }

      if (choice.startsWith("Custom")) {
        const instructions = await ctx.ui.editor("Custom review instructions", "");
        const custom = instructions?.trim();
        if (!custom) return;
        await sendReview(pi, custom);
      }
    },
  });
}
