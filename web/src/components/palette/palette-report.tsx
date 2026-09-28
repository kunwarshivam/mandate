import type { ReactNode } from "react";
import Link from "next/link";
import { LimitRail } from "@/components/domain/envelope";
import { SourceTag } from "@/components/domain/mode";
import { SignedMoney } from "@/components/domain/money";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import { type Vision, composite, hexOf, parseOklch, simulateHex, toHex } from "@/lib/color";
import { checkCvd, checkPalette } from "@/lib/contrast";
import { CVD_DISTINCT, CVD_VISIONS, KUMO_PAIRS, type KumoScope, REQUIREMENT } from "@/lib/contrast-pairs";
import { dec } from "@/lib/decimal";
import { PALETTE, RAMPS, STEPS, TOKEN_NAMES, type TokenName, hatchInk } from "@/lib/palette";
import { TOKEN_ROLES } from "@/lib/tokens";
import { cn } from "@/lib/utils";

function Block({ title, lead, children }: { title: string; lead?: ReactNode; children: ReactNode }) {
  const id = `palette-${title.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`;
  return (
    <section className="grid gap-(--block-gap)" aria-labelledby={id}>
      <div className="grid gap-1 border-b-2 border-foreground pb-2">
        <h2 id={id} className="text-title">
          {title}
        </h2>
        {lead ? <p className="max-w-prose text-muted-foreground">{lead}</p> : null}
      </div>
      {children}
    </section>
  );
}

function Verdict({ pass }: { pass: boolean }) {
  return pass ? <span>Pass</span> : <span className="bg-ink px-1.5 label-caps text-ink-foreground">Fails</span>;
}

const VISION_LABEL: Record<Vision, string> = { normal: "Normal", deuteranopia: "Deuteranopia", protanopia: "Protanopia", tritanopia: "Tritanopia" };

const SCOPE_LABEL: Record<KumoScope, string> = {
  root: "The page",
  navy: "Navy, the sidebar's account block",
  field: "The mandate's brass tint",
  ink: "Ink, the Stop control",
};

function Preview() {
  return (
    <div className="grid content-start gap-(--seam) bg-background p-(--seam) text-foreground sm:grid-cols-2">
      <div className="grid gap-1 bg-lapis px-3 py-3 text-lapis-foreground">
        <span className="label-caps text-lapis-muted">Account · paper</span>
        <span className="font-display text-[2rem] leading-none font-bold tabular">$10,123.45</span>
        <span className="text-sm text-lapis-muted">Two agents running inside their mandates.</span>
      </div>
      <div className="grid gap-3 border-t-4 border-mandate-edge bg-mandate px-3 pt-2 pb-3 text-mandate-foreground">
        <p className="font-display text-base leading-none font-extrabold text-mandate-strong uppercase">Your mandate</p>
        <LimitRail rail={{ key: "hold", label: "Total holdings", used: dec("1608.09"), cap: dec("2000"), atCap: "No new buys" }} />
      </div>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 bg-card px-3 py-3 text-sm">
        <SignedMoney value="120.05" />
        <SignedMoney value="-60.80" />
        <span className="font-mono tabular">0.00 10 100 1,000</span>
      </div>
      <div className="grid gap-2 bg-mandate-soft px-3 py-2 text-sm">
        <p className="flex flex-wrap items-center gap-2">
          <SourceTag source="mandate" />
          <span className="font-bold">Drawdown ladder</span>
          <span className="text-muted-foreground">since 14:02</span>
        </p>
      </div>
      <div className="flex flex-wrap items-center gap-2 bg-card px-3 py-3 sm:col-span-2">
        <EnvironmentBadge environment="paper" />
        <span className="inline-flex h-8 items-center bg-ink px-3 text-sm font-bold text-ink-foreground">Stop</span>
        <span className="text-sm text-muted-foreground">The kill switch has a colour of its own, used nowhere else; it appears in the Stop sheet.</span>
      </div>
    </div>
  );
}

/** Kumo's own role names, drawn in each surface scope so the remap in placard-kumo.css shows. */
function KumoScopes() {
  const scopes: KumoScope[] = ["root", "navy", "field", "ink"];
  return (
    <div className="grid gap-(--seam) sm:grid-cols-2">
      {scopes.map((scope) => (
        <div
          key={scope}
          data-surface={scope === "root" ? undefined : scope}
          className="grid gap-2 px-3 py-3"
          style={{ background: "var(--color-kumo-base)", color: "var(--text-color-kumo-default)", borderTop: "4px solid var(--color-kumo-line)" }}
        >
          <span className="font-bold" style={{ color: "var(--text-color-kumo-strong)" }}>
            {SCOPE_LABEL[scope]}
          </span>
          <span className="text-sm" style={{ color: "var(--text-color-kumo-subtle)" }}>
            Secondary text, {KUMO_PAIRS.filter((p) => p.scope === scope).length} pairs measured in palette.test.ts
          </span>
          <span className="w-fit px-2 py-1 text-sm" style={{ background: "var(--color-kumo-tint)" }}>
            A tint
          </span>
        </div>
      ))}
    </div>
  );
}

function Swatch({ color, className }: { color: string; className?: string }) {
  return <span className={cn("block h-8 border", className)} style={{ background: color }} aria-hidden />;
}

export function PaletteReport({ colourBlind }: { colourBlind: boolean }) {
  const contrast = checkPalette();
  const cvd = checkCvd();
  const passing = contrast.filter((r) => r.passWcag).length;
  const hatchOnCard = hexOf(composite(PALETTE.tokens[PALETTE.hatch.ref].value, PALETTE.hatch.alpha, PALETTE.tokens.card.value));

  return (
    <div className="grid gap-(--section-gap)">
      <header className="grid gap-3">
        <h1 className="text-title sm:text-display">Palette</h1>
        <p className="max-w-prose text-muted-foreground">
          Development only. {PALETTE.name}: {PALETTE.summary} The reasoning, citations and history are in <code>web/COLOR.md</code>.
        </p>
        <nav aria-label="Palette" className="flex flex-wrap items-center gap-(--seam)">
          <Link href={`/palette?cvd=${colourBlind ? "0" : "1"}`} className="inline-flex h-11 items-center px-3 text-primary underline underline-offset-2">
            Colour-blind friendly: {colourBlind ? "on" : "off"}
          </Link>
          <span className="text-caption text-muted-foreground">Alt+Shift+C toggles it anywhere.</span>
        </nav>
      </header>

      <Block title="In use" lead="The account on navy, the mandate on a pale brass tint under a brass rule, results in the status family, and figures with a plain zero.">
        <Preview />
      </Block>

      <Block title="Kumo surfaces" lead="Kumo components read their own role names; placard-kumo.css points each one at a palette token, and re-points them inside each data-surface region.">
        <KumoScopes />
      </Block>

      <Block title="Ramps" lead="OKLCH, eleven steps per hue at constant hue, one lightness curve for every ramp, chroma rising to a hump in the middle. Status ramps share lightness and chroma and differ only in hue.">
        <div className="-mx-(--page-x) overflow-x-auto px-(--page-x)">
          <table className="w-full min-w-[60rem] text-caption">
            <caption className="sr-only">Colour ramps</caption>
            <thead>
              <tr className="border-b-2 border-foreground text-left">
                <th scope="col" className="py-2 pr-3 label-caps">
                  Ramp
                </th>
                {STEPS.map((s) => (
                  <th key={s} scope="col" className="px-0.5 py-2 label-caps">
                    {s}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {Object.values(RAMPS).map((ramp) => (
                <tr key={ramp.id} className="border-b align-top">
                  <th scope="row" className="py-2 pr-3 text-left font-normal">
                    <span className="block font-bold">{ramp.name}</span>
                    <span className="block text-muted-foreground">
                      h {ramp.hue}. {ramp.use}
                    </span>
                  </th>
                  {STEPS.map((s) => {
                    const { l, c } = parseOklch(ramp.steps[s]);
                    return (
                      <td key={s} className="px-0.5 py-2">
                        <Swatch color={ramp.steps[s]} />
                        <span className="block font-mono tabular" translate="no">
                          {toHex(ramp.steps[s])}
                        </span>
                        <span className="block font-mono text-muted-foreground tabular">
                          {l} {c}
                        </span>
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Block>

      <Block title="Semantic tokens" lead="Components use these names only; each maps to a ramp step.">
        <div className="-mx-(--page-x) overflow-x-auto px-(--page-x)">
          <table className="w-full min-w-[48rem] text-sm">
            <caption className="sr-only">Semantic tokens</caption>
            <thead>
              <tr className="border-b-2 border-foreground text-left">
                {["Swatch", "Token", "Ramp step", "OKLCH", "Hex", "Role"].map((h) => (
                  <th key={h} scope="col" className="py-2 pr-3 label-caps">
                    {h}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {TOKEN_NAMES.map((n: TokenName) => (
                <tr key={n} className="border-b">
                  <td className="w-12 py-1.5 pr-3">
                    <Swatch color={`var(--${n})`} className="h-6" />
                  </td>
                  <th scope="row" className="py-1.5 pr-3 text-left font-mono text-caption font-bold">
                    --{n}
                  </th>
                  <td className="py-1.5 pr-3 font-mono text-caption">{PALETTE.tokens[n].ref}</td>
                  <td className="py-1.5 pr-3 font-mono text-caption">{PALETTE.tokens[n].value}</td>
                  <td className="py-1.5 pr-3 font-mono text-caption">{toHex(PALETTE.tokens[n].value)}</td>
                  <td className="py-1.5 text-caption text-muted-foreground">{TOKEN_ROLES[n].role}</td>
                </tr>
              ))}
              <tr className="border-b">
                <td className="py-1.5 pr-3">
                  <span className="hatch block h-6 border-2 border-lapis bg-card" aria-hidden />
                </td>
                <th scope="row" className="py-1.5 pr-3 text-left font-mono text-caption font-bold">
                  --hatch-ink
                </th>
                <td className="py-1.5 pr-3 font-mono text-caption">
                  {PALETTE.hatch.ref} at {PALETTE.hatch.alpha}
                </td>
                <td className="py-1.5 pr-3 font-mono text-caption">{hatchInk()}</td>
                <td className="py-1.5 pr-3 font-mono text-caption">{hatchOnCard} on card</td>
                <td className="py-1.5 text-caption text-muted-foreground">The paper hatch lines</td>
              </tr>
            </tbody>
          </table>
        </div>
      </Block>

      <Block
        title="Contrast"
        lead={`WCAG 2.2 ratio for every semantic pair: ${passing} of ${contrast.length} pass. Body text needs ${REQUIREMENT.body.wcag}:1 and marks ${REQUIREMENT.mark.wcag}:1. APCA (Lc ${REQUIREMENT.body.apca} for body text, Lc ${REQUIREMENT.mark.apca} for marks) is checked in the tests, not here: apca-w3 is a dev dependency and never ships.`}
      >
        <div className="-mx-(--page-x) overflow-x-auto px-(--page-x)">
          <table className="w-full min-w-[46rem] text-sm">
            <caption className="sr-only">Contrast by pair</caption>
            <thead>
              <tr className="border-b-2 border-foreground text-left">
                {["Sample", "Pair", "Use", "Kind", "WCAG", "Result"].map((h) => (
                  <th key={h} scope="col" className="py-2 pr-3 label-caps">
                    {h}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {contrast.map((r) => (
                <tr key={`${r.fg}-${r.bg}-${r.kind}-${r.use}`} className="border-b">
                  <td className="py-1.5 pr-3">
                    <span className="inline-flex h-7 w-14 items-center justify-center border font-bold" style={{ background: `var(--${r.bg})`, color: `var(--${r.fg})` }}>
                      {r.kind === "mark" ? <span className="block h-3 w-8" style={{ background: `var(--${r.fg})` }} /> : "Aa"}
                    </span>
                  </td>
                  <th scope="row" className="py-1.5 pr-3 text-left font-mono text-caption font-normal">
                    {r.fg} / {r.bg}
                  </th>
                  <td className="py-1.5 pr-3 text-caption text-muted-foreground">{r.use}</td>
                  <td className="py-1.5 pr-3 text-caption">{r.kind}</td>
                  <td className="py-1.5 pr-3 font-mono tabular">{r.ratio.toFixed(2)}:1</td>
                  <td className="py-1.5 text-caption">
                    <Verdict pass={r.passWcag} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Block>

      <Block
        title="Colour vision"
        lead={`Machado 2009 simulation at full severity. Distance is OKLab ΔE; two things that must never be confused need ${CVD_DISTINCT} under each simulated vision. Checks marked information show where a hue is not relied on.`}
      >
        <div className="-mx-(--page-x) overflow-x-auto px-(--page-x)">
          <table className="w-full min-w-[56rem] text-sm">
            <caption className="sr-only">Colour-vision checks</caption>
            <thead>
              <tr className="border-b-2 border-foreground text-left">
                <th scope="col" className="py-2 pr-3 label-caps">
                  Check
                </th>
                {(["normal", ...CVD_VISIONS] as Vision[]).map((v) => (
                  <th key={v} scope="col" className="py-2 pr-3 label-caps">
                    {VISION_LABEL[v]}
                  </th>
                ))}
                <th scope="col" className="py-2 label-caps">
                  Result
                </th>
              </tr>
            </thead>
            <tbody>
              {cvd.map((c) => {
                const a = PALETTE.tokens[c.a].value;
                const b = PALETTE.tokens[c.b].value;
                return (
                  <tr key={`${c.a}-${c.b}`} className="border-b align-top">
                    <th scope="row" className="py-2 pr-3 text-left font-normal">
                      <span className="block font-bold">{c.what}</span>
                      <span className="block font-mono text-caption text-muted-foreground">
                        {c.a} / {c.b}
                      </span>
                      <span className="block text-caption text-muted-foreground">{c.why}</span>
                    </th>
                    {(["normal", ...CVD_VISIONS] as Vision[]).map((v) => (
                      <td key={v} className="py-2 pr-3">
                        <span className="flex">
                          <Swatch color={v === "normal" ? a : simulateHex(a, v)} className="w-8" />
                          <Swatch color={v === "normal" ? b : simulateHex(b, v)} className="w-8" />
                        </span>
                        <span className="font-mono text-caption tabular">{(v === "normal" ? c.normal : c.byVision[v]).toFixed(3)}</span>
                      </td>
                    ))}
                    <td className="py-2 text-caption">{c.required ? <Verdict pass={c.pass} /> : <span className="text-muted-foreground">Information{c.pass ? ", apart" : ", close"}</span>}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </Block>
    </div>
  );
}
