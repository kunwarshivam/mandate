"use client";

import { type ComponentProps, type MouseEvent, type ReactNode, useId } from "react";
import Link from "next/link";
import { cn } from "@/lib/utils";
import { useDesktopStyle } from "./desktop-style";
import { PIXEL, RAISED } from "./letter";

export const WINDOW_BUTTON = cn(RAISED, "grid size-5 place-items-center bg-muted text-xs leading-none text-foreground");

const PRESS = "cursor-pointer active:border-t-foreground/60 active:border-l-foreground/60 active:border-r-card active:border-b-card outline-none focus-visible:outline-1 focus-visible:outline-dotted focus-visible:-outline-offset-4 focus-visible:outline-foreground";

/** The title bar's glyphs, as 8 by 8 pixel drawings. */
const GLYPHS = {
  minimize: [[1, 6, 5, 2]],
  maximize: [
    [0, 0, 8, 2],
    [0, 2, 1, 6],
    [7, 2, 1, 6],
    [0, 7, 8, 1],
  ],
  restore: [
    [2, 0, 6, 1],
    [7, 1, 1, 4],
    [0, 3, 6, 2],
    [0, 5, 1, 3],
    [5, 5, 1, 3],
    [0, 7, 6, 1],
  ],
  close: [
    [0, 0, 2, 1],
    [6, 0, 2, 1],
    [1, 1, 2, 1],
    [5, 1, 2, 1],
    [2, 2, 4, 1],
    [3, 3, 2, 2],
    [2, 5, 4, 1],
    [1, 6, 2, 1],
    [5, 6, 2, 1],
    [0, 7, 2, 1],
    [6, 7, 2, 1],
  ],
} as const;

export function Glyph({ name }: { name: keyof typeof GLYPHS }) {
  return (
    <svg aria-hidden viewBox="0 0 8 8" shapeRendering="crispEdges" className="size-2 fill-current">
      {GLYPHS[name].map(([x, y, w, h]) => (
        <rect key={`${x}.${y}`} x={x} y={y} width={w} height={h} />
      ))}
    </svg>
  );
}

/** A Mac window's boxes, 13 pixels square: close is empty, zoom holds a smaller box, collapse a bar. */
const BOXES = {
  close: [],
  zoom: [
    [1, 1, 6, 1],
    [1, 2, 1, 5],
    [6, 2, 1, 5],
    [1, 6, 6, 1],
  ],
  collapse: [
    [1, 5, 9, 1],
    [1, 7, 9, 1],
  ],
} as const;

function MacBox({ name }: { name: keyof typeof BOXES }) {
  return (
    <svg aria-hidden viewBox="0 0 11 13" shapeRendering="crispEdges" className="h-[13px] w-[11px] fill-current">
      {BOXES[name].map(([x, y, w, h]) => (
        <rect key={`${x}.${y}`} x={x} y={y} width={w} height={h} />
      ))}
    </svg>
  );
}

export type TitleControl = { label: string; onClick: (e: MouseEvent<HTMLButtonElement>) => void } | { label: string; href: string };

export type TitleControls = { minimize?: TitleControl; maximize?: TitleControl & { restore?: boolean }; close?: TitleControl };

function Control({ control, className, children }: { control: TitleControl; className: string; children: ReactNode }) {
  if ("href" in control)
    return (
      <Link href={control.href} aria-label={control.label} className={className}>
        {children}
      </Link>
    );
  return (
    <button type="button" tabIndex={-1} aria-label={control.label} onClick={control.onClick} className={className}>
      {children}
    </button>
  );
}

const MAC_BOX = "relative grid h-[13px] w-[13px] shrink-0 cursor-pointer place-items-center border border-foreground bg-card text-foreground ring-2 ring-card outline-none active:bg-foreground active:text-card focus-visible:outline-1 focus-visible:outline-dotted focus-visible:outline-offset-2 focus-visible:outline-foreground";

/** System 7's title bar: pinstripes, the close box at the left, zoom and collapse at the right, all gone while the window is behind. */
function MacTitleBar({ title, icon, inactive, controls, className, ...rest }: { title: string; icon?: ReactNode; inactive?: boolean; controls?: TitleControls } & ComponentProps<"div">) {
  const stripes = useId();
  const decorative = controls === undefined;
  const { close, maximize, minimize } = controls ?? {};
  return (
    <div className={cn("relative grid h-6 shrink-0 grid-cols-[1fr_auto_1fr] items-center gap-2 border-b border-foreground bg-card px-2 text-[0.9375rem] font-semibold text-foreground", PIXEL, className)} data-slot="title-bar" {...rest}>
      {!inactive && (
        <svg aria-hidden className="pointer-events-none absolute inset-x-1 top-[5px] h-[13px] w-[calc(100%-0.5rem)]" shapeRendering="crispEdges">
          <defs>
            <pattern id={stripes} width="2" height="2" patternUnits="userSpaceOnUse">
              <rect width="2" height="1" className="fill-foreground" />
            </pattern>
          </defs>
          <rect width="100%" height="100%" fill={`url(#${stripes})`} />
        </svg>
      )}
      <span className="flex">
        {!inactive && decorative && (
          <span aria-hidden className={MAC_BOX}>
            <MacBox name="close" />
          </span>
        )}
        {!inactive && close && (
          <Control control={close} className={MAC_BOX}>
            <MacBox name="close" />
          </Control>
        )}
      </span>
      <span className="relative flex min-w-0 items-center gap-1.5 bg-card px-2">
        {icon}
        <span className="truncate">{title}</span>
      </span>
      <span className="flex justify-end gap-2">
        {!inactive && decorative && (
          <>
            <span aria-hidden className={MAC_BOX}>
              <MacBox name="zoom" />
            </span>
            <span aria-hidden className={MAC_BOX}>
              <MacBox name="collapse" />
            </span>
          </>
        )}
        {!inactive && maximize && (
          <Control control={maximize} className={cn(MAC_BOX, "max-sm:hidden")}>
            <MacBox name="zoom" />
          </Control>
        )}
        {!inactive && minimize && (
          <Control control={minimize} className={MAC_BOX}>
            <MacBox name="collapse" />
          </Control>
        )}
      </span>
    </div>
  );
}

/** Windows 98's: dark while its window is in front, grey behind, with the three buttons at the right. */
function WindowsTitleBar({ title, icon, inactive, controls, className, ...rest }: { title: string; icon?: ReactNode; inactive?: boolean; controls?: TitleControls } & ComponentProps<"div">) {
  const { close, maximize, minimize } = controls ?? {};
  return (
    <div className={cn("flex h-7 shrink-0 items-center justify-between gap-3 ps-1.5 pe-0.5 text-[0.9375rem] text-card", inactive ? "bg-muted-foreground" : "bg-foreground", PIXEL, className)} data-slot="title-bar" {...rest}>
      <span className="flex min-w-0 items-center gap-1.5">
        {icon}
        <span className="truncate">{title}</span>
      </span>
      {controls === undefined ? (
        <span aria-hidden className="flex shrink-0 gap-0.5">
          {(["minimize", "maximize", "close"] as const).map((g) => (
            <span key={g} className={cn(WINDOW_BUTTON, g === "close" && "ms-0.5")}>
              <Glyph name={g} />
            </span>
          ))}
        </span>
      ) : (
        <span className="flex shrink-0 gap-0.5">
          {minimize && (
            <Control control={minimize} className={cn(WINDOW_BUTTON, PRESS)}>
              <Glyph name="minimize" />
            </Control>
          )}
          {maximize && (
            <Control control={maximize} className={cn(WINDOW_BUTTON, PRESS, "max-sm:hidden")}>
              <Glyph name={maximize.restore ? "restore" : "maximize"} />
            </Control>
          )}
          {close && (
            <Control control={close} className={cn(WINDOW_BUTTON, PRESS, "ms-0.5")}>
              <Glyph name="close" />
            </Control>
          )}
        </span>
      )}
    </div>
  );
}

/**
 * A window's title bar, in the desktop's style. Without `controls` its buttons are drawn and do
 * nothing; with them, only the ones given appear. Title bar buttons are pointer affordances, out of
 * the tab cycle, so a window's content comes first in the keyboard order (`e2e/landing.spec.ts`); a
 * link (the sign-in window's close box) stays in it, being the window's only way out.
 */
export function TitleBar(props: { title: string; icon?: ReactNode; inactive?: boolean; controls?: TitleControls } & ComponentProps<"div">) {
  return useDesktopStyle() === "mac" ? <MacTitleBar {...props} /> : <WindowsTitleBar {...props} />;
}
