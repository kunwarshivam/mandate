import type { ComponentProps } from "react";
import type NumberFlow from "@number-flow/react";

/**
 * jsdom cannot attach Number Flow's declarative shadow root, so its fallback copy and stylesheet
 * would land in the light DOM. This stand-in draws the formatted value as Number Flow's parts, each a
 * span named by `data-part`, and keeps the props tests read as attributes.
 */
export function NumberFlowMock({ value, prefix = "", suffix = "", locales, format, animated = true, className, ...rest }: ComponentProps<typeof NumberFlow>) {
  const parts = new Intl.NumberFormat(locales, format).formatToParts(Number(value));
  const fraction = parts.findIndex((p) => p.type === "decimal");
  const whole = (fraction < 0 ? parts : parts.slice(0, fraction)).map((p) => p.value).join("");
  const cents = fraction < 0 ? "" : parts.slice(fraction).map((p) => p.value).join("");
  return (
    <span data-number-flow="" data-animated={animated ? "" : undefined} className={className} aria-hidden={rest["aria-hidden"]}>
      <span data-part="integer">{prefix + whole}</span>
      {cents ? <span data-part="fraction">{cents}</span> : null}
      {suffix}
    </span>
  );
}
