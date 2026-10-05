"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { percent, usd } from "@/lib/format";
import { useRuntime } from "@/lib/mock-runtime";
import { CompiledReview } from "./compiled-review";
import { Confirmation } from "./confirmation";
import { type Answers, LOSS_CEILING, SECTION_KEYS, type SectionKey, type Source, compile, lossCeilingUsd, read, readGoal, readLoss, readMoney, readSymbols } from "./draft";
import { DescribeStep, QuestionStep, STEP_HEADING, StartStep } from "./goal-steps";

type Step = "start" | "money" | "goal" | "loss" | "describe" | "review" | "confirmed";

const NONE_CONFIRMED: Record<SectionKey, boolean> = { money: false, limits: false, autonomy: false, universe: false };
const NO_ANSWERS: Answers = { money: "", goal: "", loss: "" };

/**
 * The mandate-writing prototype (brief A0 → A2 → A5) on fixture rules. Everything lives in this
 * component's state: nothing is fetched, stored or sent, and leaving the page forgets it.
 */
export function NewAgentPrototype() {
  const { now } = useRuntime();
  const [step, setStep] = useState<Step>("start");
  const [path, setPath] = useState<"questions" | "description">("questions");
  const [answers, setAnswers] = useState<Answers>(NO_ANSWERS);
  const [description, setDescription] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [symbolsText, setSymbolsText] = useState("");
  const [symbolsError, setSymbolsError] = useState<string | null>(null);
  const [confirmed, setConfirmed] = useState(NONE_CONFIRMED);
  const [confirmedAt, setConfirmedAt] = useState<string | null>(null);

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
  const draft = useMemo(() => {
    if (step !== "review" && step !== "confirmed") return null;
    const parsed = read(path === "questions" ? { kind: "questions", answers } : { kind: "description", text: description });
    return parsed.ok ? compile(parsed.value, symbols) : null;
  }, [step, path, answers, description, symbols]);

  const go = (next: Step) => {
    setError(null);
    setStep(next);
  };
  const answer = (key: keyof Answers) => (value: string) => {
    setAnswers((a) => ({ ...a, [key]: value }));
    setError(null);
  };
  const toReview = () => {
    const parsed = read(source);
    if (!parsed.ok) {
      setError(parsed.error);
      return;
    }
    setConfirmed(NONE_CONFIRMED);
    go("review");
  };
  const allocation = readMoney(answers.money);

  switch (step) {
    case "start":
      return (
        <div ref={root} className="mx-auto w-full max-w-2xl reveal" key={step}>
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
            symbolsText={symbolsText}
            symbolsError={symbolsError}
            onSymbols={(value) => {
              setSymbolsText(value);
              setSymbolsError(null);
              setConfirmed((c) => (c.universe ? { ...c, universe: false } : c));
            }}
            onConfirm={(key) => {
              if (key === "universe") {
                const parsed = readSymbols(symbolsText);
                if (!parsed.ok) {
                  setSymbolsError(parsed.error);
                  return false;
                }
                if (parsed.value.length === 0) {
                  setSymbolsError("Add at least one symbol. Only you choose what it may trade.");
                  return false;
                }
              }
              setConfirmed((c) => ({ ...c, [key]: true }));
              return true;
            }}
            onUndo={(key) => setConfirmed((c) => ({ ...c, [key]: false }))}
            onBack={() => {
              setConfirmed(NONE_CONFIRMED);
              go(path === "questions" ? "money" : "describe");
            }}
            onDeploy={() => {
              if (!SECTION_KEYS.every((k) => confirmed[k])) return;
              setConfirmedAt(now);
              go("confirmed");
            }}
          />
        </div>
      );
    case "confirmed":
      if (!draft || !confirmedAt) return null;
      return (
        <div ref={root} className="mx-auto w-full max-w-3xl reveal" key={step}>
          <Confirmation
            draft={draft}
            confirmedAt={confirmedAt}
            onStartOver={() => {
              setAnswers(NO_ANSWERS);
              setDescription("");
              setSymbolsText("");
              setSymbolsError(null);
              setConfirmed(NONE_CONFIRMED);
              setConfirmedAt(null);
              go("start");
            }}
          />
        </div>
      );
    default: {
      const unhandled: never = step;
      throw new Error(`unhandled step ${String(unhandled)}`);
    }
  }
}
