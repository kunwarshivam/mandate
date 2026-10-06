"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { claimedBy, unallocatedUsd } from "@/lib/fixture-journey";
import { percent, usd } from "@/lib/format";
import { useRuntime } from "@/lib/mock-runtime";
import { CompiledReview, type SectionErrors } from "./compiled-review";
import { Confirmation } from "./confirmation";
import {
  type Answers,
  LOSS_CEILING,
  NO_STRATEGY,
  SECTION_KEYS,
  type SectionKey,
  type Source,
  type Strategy,
  compile,
  lossCeilingUsd,
  mandateFrom,
  read,
  readGoal,
  readLoss,
  readMoney,
  readStrategy,
  readSymbols,
} from "./draft";
import { DescribeStep, QuestionStep, STEP_HEADING, StartStep } from "./goal-steps";

type Step = "start" | "money" | "goal" | "loss" | "describe" | "review" | "confirm";

const NONE_CONFIRMED: Record<SectionKey, boolean> = { money: false, limits: false, strategy: false, autonomy: false, universe: false };
const NO_ANSWERS: Answers = { money: "", goal: "", loss: "" };

/**
 * Setting up an agent (brief A0 → A2 → A5) on the fixture workspace. The answers live in this
 * component's state until the owner confirms A5 with a passkey; only then does anything reach the
 * runtime, which repeats V-002 and V-006 when it applies the deployment.
 */
export function NewAgentFlow() {
  const { ws } = useRuntime();
  const [step, setStep] = useState<Step>("start");
  const [path, setPath] = useState<"questions" | "description">("questions");
  const [answers, setAnswers] = useState<Answers>(NO_ANSWERS);
  const [description, setDescription] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [symbolsText, setSymbolsText] = useState("");
  const [strategy, setStrategy] = useState<Strategy>(NO_STRATEGY);
  const [errors, setErrors] = useState<SectionErrors>({});
  const [confirmed, setConfirmed] = useState(NONE_CONFIRMED);
  const [attempt, setAttempt] = useState(0);

  const root = useRef<HTMLDivElement>(null);
  const firstStep = useRef(true);
  useEffect(() => {
    if (firstStep.current) {
      firstStep.current = false;
      return;
    }
    root.current?.querySelector<HTMLElement>(`#${STEP_HEADING}`)?.focus();
  }, [step]);

  const source: Source = path === "questions" ? { kind: "questions", answers } : { kind: "description", text: description };
  const symbols = useMemo(() => {
    const parsed = readSymbols(symbolsText);
    return parsed.ok ? parsed.value : [];
  }, [symbolsText]);
  const chosen = useMemo(() => {
    const parsed = readStrategy(strategy);
    return parsed.ok ? parsed.value : null;
  }, [strategy]);
  const draft = useMemo(() => {
    if (step !== "review" && step !== "confirm") return null;
    const parsed = read(path === "questions" ? { kind: "questions", answers } : { kind: "description", text: description });
    return parsed.ok ? compile(parsed.value, symbols, chosen) : null;
  }, [step, path, answers, description, symbols, chosen]);
  const request = useMemo(() => (draft && step === "confirm" ? mandateFrom(draft, ws.connection.connection_id) : null), [draft, step, ws.connection.connection_id]);

  const go = (next: Step) => {
    setError(null);
    setStep(next);
  };
  const answer = (key: keyof Answers) => (value: string) => {
    setAnswers((a) => ({ ...a, [key]: value }));
    setError(null);
  };
  const unconfirm = (key: SectionKey) => {
    setConfirmed((c) => (c[key] ? { ...c, [key]: false } : c));
    setErrors((e) => ({ ...e, [key]: null }));
  };
  const toReview = () => {
    const parsed = read(source);
    if (!parsed.ok) {
      setError(parsed.error);
      return;
    }
    setConfirmed(NONE_CONFIRMED);
    setErrors({});
    go("review");
  };
  const startOver = () => {
    setAnswers(NO_ANSWERS);
    setDescription("");
    setSymbolsText("");
    setStrategy(NO_STRATEGY);
    setErrors({});
    setConfirmed(NONE_CONFIRMED);
    setAttempt((n) => n + 1);
    go("start");
  };

  /** The checks a section must pass to be confirmed; the runtime repeats V-002 and V-006 when it deploys. */
  const blocker = (key: SectionKey): string | null => {
    if (!draft) return null;
    switch (key) {
      case "money": {
        const room = unallocatedUsd(ws);
        if (draft.terms.allocationUsd > room) {
          return `Your paper account has ${usd(room)} that no agent uses, less than the ${usd(draft.terms.allocationUsd)} this agent asks for. Change your answers to use less.`;
        }
        return null;
      }
      case "strategy": {
        const parsed = readStrategy(strategy);
        return parsed.ok ? null : parsed.error;
      }
      case "universe": {
        const parsed = readSymbols(symbolsText);
        if (!parsed.ok) return parsed.error;
        if (parsed.value.length === 0) return "Add at least one symbol. Only you choose what it may trade.";
        for (const s of parsed.value) {
          const holder = claimedBy(ws, s);
          if (holder) return `${s} is already traded by ${holder.label}. One agent trades an instrument on an account; choose another.`;
        }
        return null;
      }
      case "limits":
      case "autonomy":
        return null;
      default: {
        const unhandled: never = key;
        throw new Error(`unhandled section ${String(unhandled)}`);
      }
    }
  };
  const allocation = readMoney(answers.money);

  switch (step) {
    case "start":
      return (
        <div ref={root} className="mx-auto w-full max-w-2xl reveal" key={`${step}-${attempt}`}>
          <StartStep
            onQuestions={() => {
              setPath("questions");
              go("money");
            }}
            onDescribe={() => {
              setPath("description");
              go("describe");
            }}
          />
        </div>
      );
    case "money":
      return (
        <div ref={root} className="mx-auto w-full max-w-xl reveal" key={step}>
          <QuestionStep
            index={1}
            question="How much money may this agent use?"
            label="In dollars"
            hint="Simulated money: this agent trades on paper only. It never touches more than this."
            value={answers.money}
            error={error}
            onChange={answer("money")}
            onBack={() => go("start")}
            onContinue={() => {
              const result = readMoney(answers.money);
              if (result.ok) go("goal");
              else setError(result.error);
            }}
          />
        </div>
      );
    case "goal":
      return (
        <div ref={root} className="mx-auto w-full max-w-xl reveal" key={step}>
          <QuestionStep
            index={2}
            question="What is the goal?"
            label="In your own words"
            hint="If it should stop at a level, write the level in figures. Anything a limit cannot check is kept as a note and marked not enforced."
            value={answers.goal}
            error={error}
            multiline
            onChange={answer("goal")}
            onBack={() => go("money")}
            onContinue={() => {
              const result = readGoal(answers.goal);
              if (result.ok) go("loss");
              else setError(result.error);
            }}
          />
        </div>
      );
    case "loss":
      return (
        <div ref={root} className="mx-auto w-full max-w-xl reveal" key={step}>
          <QuestionStep
            index={3}
            question="How much could you stand to lose?"
            label="In dollars, or as a percentage of the money"
            hint={
              <>
                In total, over the agent&apos;s life. When it has lost this much it closes everything and pauses. This workspace&apos;s limit: at most{" "}
                {percent(LOSS_CEILING, 0)} of the money
                {allocation.ok ? (
                  <>
                    , <span className="font-mono tabular">{usd(lossCeilingUsd(allocation.value))}</span>
                  </>
                ) : null}
                .
              </>
            }
            value={answers.loss}
            error={error}
            onChange={answer("loss")}
            onBack={() => go("goal")}
            onContinue={() => {
              if (!allocation.ok) return go("money");
              const result = readLoss(answers.loss, allocation.value);
              if (result.ok) toReview();
              else setError(result.error);
            }}
          />
        </div>
      );
    case "describe":
      return (
        <div ref={root} className="mx-auto w-full max-w-xl reveal" key={step}>
          <DescribeStep
            value={description}
            error={error}
            onChange={(value) => {
              setDescription(value);
              setError(null);
            }}
            onBack={() => go("start")}
            onContinue={toReview}
          />
        </div>
      );
    case "review":
      if (!draft) return null;
      return (
        <div ref={root} className="mx-auto w-full max-w-3xl reveal" key={step}>
          <CompiledReview
            draft={draft}
            confirmed={confirmed}
            errors={errors}
            symbolsText={symbolsText}
            strategy={strategy}
            onSymbols={(value) => {
              setSymbolsText(value);
              unconfirm("universe");
            }}
            onStrategy={(next) => {
              setStrategy(next);
              unconfirm("strategy");
            }}
            onConfirm={(key) => {
              const why = blocker(key);
              setErrors((e) => ({ ...e, [key]: why }));
              if (why) return false;
              setConfirmed((c) => ({ ...c, [key]: true }));
              return true;
            }}
            onUndo={(key) => setConfirmed((c) => ({ ...c, [key]: false }))}
            onBack={() => {
              setConfirmed(NONE_CONFIRMED);
              setErrors({});
              go(path === "questions" ? "money" : "describe");
            }}
            onContinue={() => {
              if (SECTION_KEYS.every((k) => confirmed[k])) go("confirm");
            }}
          />
        </div>
      );
    case "confirm":
      if (!draft || !request) return null;
      return (
        <div ref={root} className="mx-auto w-full max-w-3xl reveal" key={step}>
          <Confirmation draft={draft} request={request} onBack={() => go("review")} onStartOver={startOver} />
        </div>
      );
    default: {
      const unhandled: never = step;
      throw new Error(`unhandled step ${String(unhandled)}`);
    }
  }
}
