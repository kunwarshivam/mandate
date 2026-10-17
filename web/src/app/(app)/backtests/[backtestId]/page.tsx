import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { BacktestReportScreen } from "@/components/backtest/backtest-report";
import { BACKTEST_ID } from "@/lib/ids";

/** Generic on purpose: the title names the kind of record, never the version or the instrument. */
export const metadata: Metadata = { title: "Backtest" };

/** One screen for both data sources: it reads `GET /backtests/{id}` through the runtime (DEC-2400). */
export default async function BacktestPage({ params }: { params: Promise<{ backtestId: string }> }) {
  const { backtestId } = await params;
  if (!BACKTEST_ID.test(backtestId)) notFound();
  return <BacktestReportScreen backtestId={backtestId} />;
}
