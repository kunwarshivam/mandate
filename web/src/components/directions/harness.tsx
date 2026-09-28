"use client";

import { type ComponentType, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import "@fontsource-variable/archivo/wdth.css";
import "@fontsource-variable/azeret-mono";
import "@fontsource-variable/big-shoulders-display";
import "@fontsource-variable/atkinson-hyperlegible-next";
import "@fontsource-variable/funnel-sans";
import "@fontsource-variable/funnel-display";
import "./directions.css";
import { THEME_KEY } from "@/components/shell/theme-toggle";
import { APPROVAL_IDS } from "@/fixtures/workspace";
import { useRuntime } from "@/lib/mock-runtime";
import { DIRECTIONS, type DirectionId, buildDirectionCss } from "./directions";
import { KeelApproval, KeelDashboard } from "./keel";
import { PlacardApproval, PlacardDashboard } from "./placard";
import { MotionPersonality } from "./shared";
import { VernierApproval, VernierDashboard } from "./vernier";

export type Surface = "dashboard" | "approval";

const SURFACES: Array<{ id: Surface; label: string }> = [
  { id: "dashboard", label: "Dashboard" },
  { id: "approval", label: "Approval, phone" },
];

const VIEWS: Record<DirectionId, { Dashboard: ComponentType; Approval: ComponentType<{ approvalId: string }> }> = {
  vernier: { Dashboard: VernierDashboard, Approval: VernierApproval },
  placard: { Dashboard: PlacardDashboard, Approval: PlacardApproval },
  keel: { Dashboard: KeelDashboard, Approval: KeelApproval },
};

const DIRECTION_CSS = DIRECTIONS.map(buildDirectionCss).join("\n");

function isTyping(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  return /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName) || target.isContentEditable;
}

/** The prototype skill's picker: markup, classes, and behaviour are its spec, not a design choice. */
function Picker({ labels, current, onPick, onReplay, position, label }: { labels: string[]; current: number; onPick: (i: number) => void; onReplay?: () => void; position?: "top"; label: string }) {
  const nav = useRef<HTMLElement>(null);
  const highlight = useRef<HTMLSpanElement>(null);
  const items = useRef<Array<HTMLButtonElement | null>>([]);

  const move = useCallback(() => {
    const el = items.current[current];
    if (!el || !highlight.current) return;
    highlight.current.style.width = `${el.offsetWidth}px`;
    highlight.current.style.transform = `translateX(${el.offsetLeft}px)`;
  }, [current]);

  useLayoutEffect(move, [move]);

  useEffect(() => {
    window.addEventListener("resize", move);
    document.fonts?.ready.then(move);
    const frame = requestAnimationFrame(() => requestAnimationFrame(() => nav.current?.setAttribute("data-ready", "")));
    return () => {
      window.removeEventListener("resize", move);
      cancelAnimationFrame(frame);
    };
  }, [move]);

  return (
    <nav ref={nav} className="proto-picker" aria-label={label} data-position={position}>
      <span ref={highlight} className="proto-picker-highlight" aria-hidden="true" />
      {labels.map((name, i) => (
        <button
          key={name}
          ref={(el) => {
            items.current[i] = el;
          }}
          type="button"
          className="proto-picker-item"
          data-active={i === current ? "" : undefined}
          aria-current={i === current ? "true" : undefined}
          onClick={() => onPick(i)}
        >
          {name}
        </button>
      ))}
      {onReplay ? (
        <>
          <span className="proto-picker-divider" aria-hidden="true" />
          <button type="button" className="proto-picker-item proto-picker-replay" aria-label="Replay animation (R)" onClick={onReplay}>
            ↻
          </button>
        </>
      ) : null}
    </nav>
  );
}

function PhoneFrame({ index, run }: { index: number; run: number }) {
  return (
    <div className="grid justify-items-center py-6 [@media(max-height:1040px)]:h-[calc(844px*0.8+3rem)]">
      <iframe
        key={`${index}-${run}`}
        title="Approval request at phone width"
        src={`/directions?v=${index + 1}&s=approval&embed=1`}
        width={390}
        height={844}
        className="max-w-full origin-top rounded-[2.25rem] border-[10px] border-foreground bg-background [@media(max-height:1040px)]:scale-[0.8]"
      />
    </div>
  );
}

/** Keeps a framed phone view on the parent's theme when the header toggle flips it. */
function useThemeFromStorage(enabled: boolean) {
  useEffect(() => {
    if (!enabled) return;
    const onStorage = (e: StorageEvent) => {
      if (e.key === THEME_KEY) document.documentElement.classList.toggle("dark", e.newValue === "dark");
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, [enabled]);
}

export function DirectionsHarness({ initialIndex, initialSurface, embed }: { initialIndex: number; initialSurface: Surface; embed: boolean }) {
  const { ws } = useRuntime();
  const [index, setIndex] = useState(initialIndex);
  const [surface, setSurface] = useState<Surface>(initialSurface);
  const [run, setRun] = useState(0);
  const direction = DIRECTIONS[index];
  const { Dashboard, Approval } = VIEWS[direction.id];
  const approvalId = ws.approvals.some((a) => a.approval_id === APPROVAL_IDS.swingXyz) ? APPROVAL_IDS.swingXyz : (ws.approvals[0]?.approval_id ?? APPROVAL_IDS.swingXyz);

  useThemeFromStorage(embed);

  const pick = useCallback((i: number) => {
    if (i < 0 || i >= DIRECTIONS.length) return;
    setIndex(i);
    setRun((r) => r + 1);
    const url = new URL(window.location.href);
    url.searchParams.set("v", String(i + 1));
    window.history.replaceState(null, "", url);
  }, []);

  const pickSurface = useCallback((i: number) => {
    const next = SURFACES[i].id;
    setSurface(next);
    const url = new URL(window.location.href);
    url.searchParams.set("s", next);
    window.history.replaceState(null, "", url);
  }, []);

  useEffect(() => {
    if (embed) return;
    const onKey = (e: KeyboardEvent) => {
      if (isTyping(e.target) || e.metaKey || e.ctrlKey || e.altKey) return;
      const n = Number.parseInt(e.key, 10);
      if (n >= 1 && n <= DIRECTIONS.length) pick(n - 1);
      else if (e.key === "ArrowRight") pick((index + 1) % DIRECTIONS.length);
      else if (e.key === "ArrowLeft") pick((index - 1 + DIRECTIONS.length) % DIRECTIONS.length);
      else if (e.key === "r" || e.key === "R") setRun((r) => r + 1);
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [embed, index, pick]);

  return (
    <>
      <style>{DIRECTION_CSS}</style>
      <div key={`${direction.id}-${run}`} data-direction={direction.id} data-embed={embed ? "" : undefined} data-surface={surface}>
        <MotionPersonality motion={direction.motion}>
          {surface === "dashboard" ? <Dashboard /> : embed ? <Approval approvalId={approvalId} /> : <PhoneFrame index={index} run={run} />}
        </MotionPersonality>
      </div>
      {embed ? null : (
        <>
          <Picker label="Prototype surface" position="top" labels={SURFACES.map((s) => s.label)} current={SURFACES.findIndex((s) => s.id === surface)} onPick={pickSurface} />
          <Picker label="Prototype variants" labels={DIRECTIONS.map((d) => d.name)} current={index} onPick={pick} onReplay={() => setRun((r) => r + 1)} />
        </>
      )}
    </>
  );
}
