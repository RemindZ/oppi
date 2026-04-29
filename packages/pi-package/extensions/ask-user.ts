import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { getAgentDir } from "@mariozechner/pi-coding-agent";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { Editor, type EditorTheme, Key, matchesKey, Text, truncateToWidth } from "@mariozechner/pi-tui";
import { Type } from "typebox";

type AskUserOption = {
  id: string;
  label: string;
  description?: string;
};

type AskUserQuestion = {
  id: string;
  question: string;
  options?: AskUserOption[];
  allowCustom?: boolean;
  defaultOptionId?: string;
  required?: boolean;
};

type AskUserAnswer = {
  questionId: string;
  optionId?: string;
  label?: string;
  text?: string;
  skipped?: boolean;
};

export type AskUserConfig = {
  timeoutMinutes: 0 | 1 | 2 | 5 | 10;
};

type AskUserDetails = {
  title: string;
  questions: AskUserQuestion[];
  answers: AskUserAnswer[];
  cancelled: boolean;
  timedOut?: boolean;
};

export const ASK_USER_TIMEOUT_MINUTES = [0, 1, 2, 5, 10] as const;
const DEFAULT_ASK_USER_TIMEOUT_MINUTES: AskUserConfig["timeoutMinutes"] = 5;

const AskUserOptionSchema = Type.Object(
  {
    id: Type.String({ description: "Stable option id returned to the model if selected." }),
    label: Type.String({ description: "Human-readable option label." }),
    description: Type.Optional(Type.String({ description: "Optional short explanation shown below the label." })),
  },
  { additionalProperties: false },
);

const AskUserQuestionSchema = Type.Object(
  {
    id: Type.String({ description: "Stable question id." }),
    question: Type.String({ description: "Question to show the user." }),
    options: Type.Optional(Type.Array(AskUserOptionSchema, { description: "Optional predefined answers." })),
    allowCustom: Type.Optional(Type.Boolean({ description: "Allow a custom free-text answer. Defaults to true when no options are provided." })),
    defaultOptionId: Type.Optional(Type.String({ description: "Option id to preselect when present." })),
    required: Type.Optional(Type.Boolean({ description: "Whether an answer is required. Defaults to true." })),
  },
  { additionalProperties: false },
);

const AskUserParams = Type.Object(
  {
    title: Type.Optional(Type.String({ description: "Optional questionnaire title." })),
    questions: Type.Array(AskUserQuestionSchema, { description: "One or more questions to ask in one interaction." }),
  },
  { additionalProperties: false },
) as any;

type AskUserInput = {
  title?: string;
  questions: AskUserQuestion[];
};

type RenderOption = AskUserOption & { custom?: boolean; skip?: boolean };

type OppiSettingsFile = Record<string, any> & {
  oppi?: {
    askUser?: Partial<AskUserConfig>;
  };
};

function globalSettingsPath(): string {
  return join(getAgentDir(), "settings.json");
}

function projectSettingsPath(cwd: string): string {
  return join(cwd, ".pi", "settings.json");
}

function readJson(path: string): OppiSettingsFile {
  try {
    if (!existsSync(path)) return {};
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return {};
  }
}

export function coerceAskUserTimeout(value: unknown): AskUserConfig["timeoutMinutes"] {
  const numeric = Number(value);
  return (ASK_USER_TIMEOUT_MINUTES as readonly number[]).includes(numeric) ? (numeric as AskUserConfig["timeoutMinutes"]) : DEFAULT_ASK_USER_TIMEOUT_MINUTES;
}

function normalizeConfig(value: Partial<AskUserConfig> | undefined): AskUserConfig {
  return { timeoutMinutes: coerceAskUserTimeout(value?.timeoutMinutes) };
}

export function readAskUserConfig(cwd: string): AskUserConfig {
  const global = readJson(globalSettingsPath()).oppi?.askUser;
  const project = readJson(projectSettingsPath(cwd)).oppi?.askUser;
  return normalizeConfig({ ...global, ...project });
}

export function writeGlobalAskUserConfig(config: AskUserConfig): void {
  const path = globalSettingsPath();
  const data = readJson(path);
  data.oppi = data.oppi ?? {};
  data.oppi.askUser = normalizeConfig(config);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(data, null, 2)}\n`, "utf8");
}

export function askUserTimeoutLabel(minutes: AskUserConfig["timeoutMinutes"]): string {
  return minutes === 0 ? "off" : `${minutes}m`;
}

function normalizeQuestions(params: AskUserInput): AskUserQuestion[] {
  return params.questions.map((question, index) => ({
    id: String(question.id || `q${index + 1}`),
    question: String(question.question || "Question"),
    options: (question.options ?? []).map((option, optionIndex) => ({
      id: String(option.id || `option-${optionIndex + 1}`),
      label: String(option.label || option.id || `Option ${optionIndex + 1}`),
      description: option.description,
    })),
    allowCustom: question.allowCustom ?? ((question.options ?? []).length === 0),
    defaultOptionId: question.defaultOptionId,
    required: question.required !== false,
  }));
}

function answerText(answer: AskUserAnswer): string {
  if (answer.skipped) return "skipped";
  if (answer.text !== undefined) return `custom: ${answer.text}`;
  return answer.optionId ? `${answer.optionId}${answer.label ? ` (${answer.label})` : ""}` : answer.label || "answered";
}

function textResult(details: AskUserDetails): string {
  if (details.cancelled) return "User cancelled the questionnaire.";
  const prefix = details.timedOut ? "Questionnaire timed out; unanswered questions used recommended/default answers.\n" : "";
  return prefix + details.answers.map((answer) => `${answer.questionId}: ${answerText(answer)}`).join("\n");
}

function isEnterKey(data: string): boolean {
  return matchesKey(data, Key.enter) || data === "\r" || data === "\n" || data === "\r\n";
}

export default function askUserExtension(pi: ExtensionAPI) {
  const registerTool = pi.registerTool.bind(pi) as (tool: any) => void;
  registerTool({
    name: "ask_user",
    label: "ask_user",
    description: "Ask the user one or more structured questions in a single interaction and return their answers.",
    promptSnippet: "Use ask_user to ask focused clarifying questions or request explicit user decisions.",
    promptGuidelines: [
      "Use ask_user when you need user input before taking action, especially for ambiguous requirements or permission overrides.",
      "Batch related questions into one ask_user call instead of asking one at a time.",
      "Provide concrete options when possible and include allowCustom when free-form input is useful.",
      "Keep questions short and directly actionable.",
    ],
    parameters: AskUserParams,
    async execute(_toolCallId, params: AskUserInput, _signal, _onUpdate, ctx) {
      if (!ctx.hasUI) {
        const details: AskUserDetails = { title: params.title || "OPPi questions", questions: normalizeQuestions(params), answers: [], cancelled: true };
        return { content: [{ type: "text", text: "Error: ask_user requires interactive UI." }], details };
      }

      const questions = normalizeQuestions(params);
      if (questions.length === 0) {
        const details: AskUserDetails = { title: params.title || "OPPi questions", questions, answers: [], cancelled: true };
        return { content: [{ type: "text", text: "Error: no questions provided." }], details };
      }

      const title = params.title?.trim() || (questions.length === 1 ? "OPPi needs a quick answer" : "OPPi needs a tiny bit of steering");
      const config = readAskUserConfig(ctx.cwd);

      const result = await (ctx.ui.custom as any)((tui: any, theme: any, _kb: any, done: (result: AskUserDetails) => void) => {
        let current = 0;
        let selected = 0;
        let inputMode = false;
        let cached: string[] | undefined;
        const answers = new Map<string, AskUserAnswer>();
        let closed = false;
        const startedAt = Date.now();
        const timeoutMs = config.timeoutMinutes === 0 ? 0 : config.timeoutMinutes * 60_000;

        const editorTheme: EditorTheme = {
          borderColor: (s) => theme.fg("accent", s),
          selectList: {
            selectedPrefix: (t) => theme.fg("accent", t),
            selectedText: (t) => theme.fg("accent", t),
            description: (t) => theme.fg("muted", t),
            scrollInfo: (t) => theme.fg("dim", t),
            noMatch: (t) => theme.fg("warning", t),
          },
        };
        const editor = new Editor(tui, editorTheme);

        function question(): AskUserQuestion {
          return questions[current];
        }

        function optionsFor(q: AskUserQuestion): RenderOption[] {
          const options: RenderOption[] = [...(q.options ?? [])];
          if (q.allowCustom) options.push({ id: "__custom__", label: "Custom...", custom: true });
          if (!q.required) options.push({ id: "__skip__", label: "Skip", skip: true });
          return options;
        }

        function isComplete(): boolean {
          return questions.every((q) => !q.required || answers.has(q.id));
        }

        function recommendedAnswer(q: AskUserQuestion): AskUserAnswer {
          const opts = optionsFor(q);
          const preferred = opts.find((option) => option.id === q.defaultOptionId && !option.custom) ?? opts.find((option) => !option.custom && !option.skip);
          if (preferred) {
            return preferred.skip
              ? { questionId: q.id, skipped: true }
              : { questionId: q.id, optionId: preferred.id, label: preferred.label };
          }
          return q.required
            ? { questionId: q.id, text: "No recommended answer was available before timeout." }
            : { questionId: q.id, skipped: true };
        }

        function complete(cancelled: boolean, timedOut = false): void {
          if (closed) return;
          closed = true;
          if (timeout) clearTimeout(timeout);
          if (timedOut) {
            for (const q of questions) {
              if (!answers.has(q.id)) answers.set(q.id, recommendedAnswer(q));
            }
          }
          done({ title, questions, answers: Array.from(answers.values()), cancelled, timedOut });
        }

        const timeout = timeoutMs > 0 ? setTimeout(() => complete(false, true), timeoutMs) : undefined;
        timeout?.unref?.();

        function refresh(): void {
          cached = undefined;
          tui.requestRender();
        }

        function moveToNext(): void {
          if (current < questions.length - 1) {
            current++;
            selected = defaultIndex(question());
            refresh();
            return;
          }
          if (isComplete()) complete(false);
          else refresh();
        }

        function defaultIndex(q: AskUserQuestion): number {
          const opts = optionsFor(q);
          const index = opts.findIndex((option) => option.id === q.defaultOptionId);
          return Math.max(0, index);
        }

        function setAnswer(q: AskUserQuestion, option: RenderOption): void {
          if (option.skip) {
            answers.set(q.id, { questionId: q.id, skipped: true });
          } else {
            answers.set(q.id, { questionId: q.id, optionId: option.id, label: option.label });
          }
          moveToNext();
        }

        editor.onSubmit = (value) => {
          const text = value.trim();
          if (!text) return;
          answers.set(question().id, { questionId: question().id, text });
          editor.setText("");
          inputMode = false;
          moveToNext();
        };

        function handleInput(data: string): void {
          if (inputMode) {
            if (matchesKey(data, Key.escape)) {
              inputMode = false;
              editor.setText("");
              refresh();
              return;
            }
            if (isEnterKey(data)) {
              const text = editor.getExpandedText().trim();
              if (text) {
                answers.set(question().id, { questionId: question().id, text });
                editor.setText("");
                inputMode = false;
                moveToNext();
              }
              return;
            }
            editor.handleInput(data);
            refresh();
            return;
          }

          const q = question();
          const opts = optionsFor(q);
          const digit = /^[1-9]$/.test(data) ? Number(data) : undefined;
          if (digit !== undefined && digit <= opts.length) {
            const option = opts[digit - 1];
            if (option.custom) {
              inputMode = true;
              editor.setText("");
              refresh();
            } else {
              setAnswer(q, option);
            }
            return;
          }

          if (matchesKey(data, Key.up)) {
            selected = Math.max(0, selected - 1);
            refresh();
            return;
          }
          if (matchesKey(data, Key.down)) {
            selected = Math.min(opts.length - 1, selected + 1);
            refresh();
            return;
          }
          if (matchesKey(data, Key.left) && current > 0) {
            current--;
            selected = defaultIndex(question());
            refresh();
            return;
          }
          if (matchesKey(data, Key.right) && current < questions.length - 1) {
            current++;
            selected = defaultIndex(question());
            refresh();
            return;
          }
          if (isEnterKey(data)) {
            const option = opts[selected];
            if (option?.custom) {
              inputMode = true;
              editor.setText("");
              refresh();
            } else if (option) {
              setAnswer(q, option);
            }
            return;
          }
          if (matchesKey(data, Key.escape) || matchesKey(data, Key.ctrl("c"))) {
            complete(true);
          }
        }

        function render(width: number): string[] {
          if (cached) return cached;
          const q = question();
          const opts = optionsFor(q);
          const answer = answers.get(q.id);
          const lines: string[] = [];
          const add = (line = "") => lines.push(truncateToWidth(line, width));

          add(theme.fg("accent", "─".repeat(width)));
          const remainingMs = timeoutMs > 0 ? Math.max(0, timeoutMs - (Date.now() - startedAt)) : 0;
          const timeoutText = timeoutMs > 0 ? ` · timeout ${Math.max(1, Math.ceil(remainingMs / 60_000))}m` : " · timeout off";
          add(`${theme.fg("accent", theme.bold(title))} ${theme.fg("dim", `${current + 1}/${questions.length}${timeoutText}`)}`);
          if (questions.length > 1) {
            const progress = questions.map((item, index) => {
              const answered = answers.has(item.id);
              const active = index === current;
              const mark = answered ? "✓" : active ? "●" : "○";
              const pill = `${mark} ${item.id}`;
              if (active && answered) return theme.bg("selectedBg", theme.bold(theme.fg("success", pill)));
              if (active) return theme.bg("selectedBg", theme.bold(theme.fg("accent", pill)));
              return theme.fg((answered ? "success" : "dim") as any, pill);
            }).join(theme.fg("dim", "  "));
            add(progress);
          }
          add();
          add(theme.fg("text", q.question));
          if (answer) add(theme.fg("dim", `Current answer: ${answerText(answer)}`));
          add();

          for (let i = 0; i < opts.length; i++) {
            const option = opts[i];
            const prefix = i === selected ? theme.fg("accent", "> ") : "  ";
            const recommended = option.id === q.defaultOptionId && !option.custom && !option.skip;
            const tag = recommended ? ` ${theme.fg("warning", "[recommended]")}` : "";
            const label = `${i + 1}. ${option.label}${tag}`;
            add(prefix + theme.fg(i === selected ? "accent" : "text", label));
            if (option.description) add(`     ${theme.fg("muted", option.description)}`);
          }

          if (inputMode) {
            add();
            add(theme.fg("muted", "Custom answer:"));
            for (const line of editor.render(Math.max(10, width - 2))) add(` ${line}`);
          }

          add();
          add(theme.fg("dim", inputMode ? "Enter submit custom • Esc options" : "1-9 pick • ↑↓ select • ←→ question • Enter confirm • Esc cancel"));
          if (timeoutMs > 0) add(theme.fg("dim", "On timeout, answered questions are kept and unanswered questions use their recommended/default option."));
          if (current === questions.length - 1 && !isComplete()) add(theme.fg("warning", "Required questions remain unanswered."));
          add(theme.fg("accent", "─".repeat(width)));
          cached = lines;
          return lines;
        }

        selected = defaultIndex(question());
        return { render, invalidate: () => { cached = undefined; }, handleInput };
      });

      const details = result;
      return { content: [{ type: "text", text: textResult(details) }], details };
    },
    renderCall(args, theme) {
      const questions = Array.isArray(args.questions) ? args.questions : [];
      const title = typeof args.title === "string" ? args.title : "questions";
      return new Text(`${theme.fg("toolTitle", theme.bold("ask_user"))} ${theme.fg("muted", `${questions.length} question${questions.length === 1 ? "" : "s"}`)} ${theme.fg("dim", title)}`, 0, 0);
    },
    renderResult(result, _options, theme) {
      const details = result.details as AskUserDetails | undefined;
      if (!details) return new Text(theme.fg("toolOutput", "ask_user finished."), 0, 0);
      if (details.cancelled) return new Text(theme.fg("warning", "User cancelled ask_user."), 0, 0);
      const lines = details.timedOut ? [theme.fg("warning", "Timed out; used recommended/default answers.")] : [];
      lines.push(...details.answers.map((answer) => `${theme.fg("success", "✓")} ${theme.fg("accent", answer.questionId)} ${theme.fg("muted", answerText(answer))}`));
      return new Text(lines.join("\n"), 0, 0);
    },
  });
}
