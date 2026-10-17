"use client";

import type { ContentRef } from "@/fixtures/types";
import type { NewAgent } from "@/lib/fixture-journey";
import type { SetUp } from "@/lib/mock-runtime";
import type { Draft } from "./draft";

/**
 * Setting up an agent's last stretch as three steps, each its own record and each shown only once the
 * one before it is recorded (DEC-2401): create the version, confirm it (A5), deploy it to paper (B3).
 */

/** A set-up the owner has confirmed, or may have: the summary it came from can no longer change. */
export function isBound(setUp: SetUp | null): boolean {
  void setUp;
  throw new Error("Unimplemented: E10-11");
}

/** Every line the deploy step shows, in order, as the journal keeps it with the deployment. */
export function deployLines(version: ContentRef, backtestId: string | null): string[] {
  void version;
  void backtestId;
  throw new Error("Unimplemented: E10-12");
}

/** What the deploy step shows: the version, the backtest shown (or none), and paper as the only place. */
export function DeployDetails({ version, backtestId }: { version: ContentRef; backtestId: string | null }): never {
  void version;
  void backtestId;
  throw new Error("Unimplemented: E10-12");
}

/** What one step's record says, in words: what was and was not recorded. */
export function StepStatus({ which, setUp }: { which: "version" | "confirm" | "deploy"; setUp: SetUp }): never {
  void which;
  void setUp;
  throw new Error("Unimplemented: E10-11");
}

/** The three steps under the summary card. */
export function SetUpSteps(props: {
  draft: Draft;
  request: NewAgent;
  current: boolean;
  blocked: string | null;
  setUp: SetUp | null;
  onCreated: (setUpId: string) => void;
  onStartOver: () => void;
}): never {
  void props;
  throw new Error("Unimplemented: E10-11");
}
