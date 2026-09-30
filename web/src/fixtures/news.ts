/**
 * Sample headlines for the dashboard's news (DEC-217). There is no news feed yet: these are
 * fixture text about the fixture instruments, shown under a "Sample headlines" label, and never a
 * link. They report events and never say where a price is going.
 */
export interface Headline {
  id: string;
  source: string;
  at: string;
  title: string;
  symbols: string[];
}

const t = (hms: string, date = "2026-09-28") => `${date}T${hms}-04:00`;

export const HEADLINES: Headline[] = [
  { id: "n1", source: "Sample Wire", at: t("13:48:00"), title: "Bitcoin trading volume eases through the afternoon ahead of the monthly options expiry", symbols: ["BTC/USD"] },
  { id: "n2", source: "Fixture Daily", at: t("12:30:00"), title: "XYZ sets its third-quarter results call for October 22", symbols: ["XYZ"] },
  { id: "n3", source: "Sample Wire", at: t("11:15:00"), title: "QRS names a new chief financial officer, effective November 1", symbols: ["QRS"] },
  { id: "n4", source: "Market Notes", at: t("09:40:00"), title: "LMN completes the expansion of its Ohio distribution centre", symbols: ["LMN"] },
  { id: "n5", source: "Fixture Daily", at: t("08:05:00"), title: "ABC raises its quarterly dividend to 24 cents a share", symbols: ["ABC"] },
  { id: "n6", source: "Market Notes", at: t("18:20:00", "2026-09-27"), title: "Crypto exchanges report steady weekend activity across major pairs", symbols: ["BTC/USD"] },
];

/** The headlines about instruments on the account or in a mandate, newest first. */
export function headlinesFor(symbols: Set<string>, limit = 5): Headline[] {
  return HEADLINES.filter((h) => h.symbols.some((s) => symbols.has(s)))
    .sort((a, b) => Date.parse(b.at) - Date.parse(a.at))
    .slice(0, limit);
}
