/**
 * Each fixture agent's journaled fills and finished orders. The fills are chosen so that they
 * reproduce the agent's positions (quantity and average cost), its realized P&L, and so its
 * equity; `history.test.ts` checks that. Protective legs are canceled when the position they
 * protected is sold. Fixture values, not a record.
 */
import { INSTRUMENTS } from "./mandates";
import type { Fill, PastOrder } from "./types";

const t = (hms: string, date: string) => `${date}T${hms}-04:00`;

type Instrument = (typeof INSTRUMENTS)[keyof typeof INSTRUMENTS];

interface Entry {
  cid: string;
  fill?: string;
  instrument: Instrument;
  side: "buy" | "sell";
  qty: string;
  limit: string | null;
  stop: string | null;
  purpose: PastOrder["purpose"];
  tif: PastOrder["time_in_force"];
  submitted: string;
  closed: string;
  state: PastOrder["state"];
  /** Fill price for a filled order. */
  at?: string;
  note?: string;
}

function build(entries: Entry[]): { past_orders: PastOrder[]; fills: Fill[] } {
  const past_orders: PastOrder[] = [];
  const fills: Fill[] = [];
  for (const e of entries) {
    const filled = e.state === "Filled";
    if (filled && (!e.fill || !e.at)) throw new Error(`filled order ${e.cid} needs a fill`);
    past_orders.push({
      client_order_id: e.cid,
      instrument: e.instrument,
      side: e.side,
      qty: e.qty,
      filled_qty: filled ? e.qty : "0",
      limit_price: e.limit,
      stop_price: e.stop,
      purpose: e.purpose,
      state: e.state,
      submitted_at: e.submitted,
      time_in_force: e.tif,
      closed_at: e.closed,
      note: e.note ?? (filled ? "Filled in full." : ""),
    });
    if (filled && e.fill && e.at) {
      fills.push({ fill_id: e.fill, client_order_id: e.cid, instrument: e.instrument, side: e.side, qty: e.qty, price: e.at, at: e.closed });
    }
  }
  past_orders.sort((a, b) => Date.parse(b.submitted_at) - Date.parse(a.submitted_at));
  fills.sort((a, b) => Date.parse(a.at) - Date.parse(b.at));
  return { past_orders, fills };
}

const SOLD = "Canceled: the position it protected was sold.";

export const BTC_HISTORY = build([
  { cid: "cid_01JCH2D6WPCN2X1GS63CH1FYJN", fill: "fil_01JCBDBE6FAPGV00P3EF0YVV15", instrument: INSTRUMENTS.BTC, side: "buy", qty: "0.02", limit: "56420", stop: null, purpose: "open", tif: "gtc", submitted: t("14:20:01", "2026-09-21"), closed: t("14:20:06", "2026-09-21"), state: "Filled", at: "56400" },
  { cid: "cid_01JCQMA854PQ65D1H86X2AN6A6", instrument: INSTRUMENTS.BTC, side: "sell", qty: "0.02", limit: "51628.56", stop: "51888", purpose: "protective", tif: "gtc", submitted: t("14:20:10", "2026-09-21"), closed: t("03:10:45", "2026-09-22"), state: "Canceled", note: SOLD },
  { cid: "cid_01JCAN5BQPGGDE0CVG1S1EDS7Z", fill: "fil_01JCVQ4KJ09YTSRV7R0RBXK4HS", instrument: INSTRUMENTS.BTC, side: "sell", qty: "0.02", limit: "55150", stop: null, purpose: "discretionary_exit", tif: "gtc", submitted: t("03:10:40", "2026-09-22"), closed: t("03:10:44", "2026-09-22"), state: "Filled", at: "55172" },
  { cid: "cid_01JCKF37QK10X9WQQDBG1Y5WYE", fill: "fil_01JCTNN6W959RCS7YM8A4JRVYH", instrument: INSTRUMENTS.BTC, side: "buy", qty: "0.06", limit: "55020", stop: null, purpose: "open", tif: "gtc", submitted: t("16:45:08", "2026-09-22"), closed: t("16:45:12", "2026-09-22"), state: "Filled", at: "55000" },
  { cid: "cid_01JC9EVY6RVM89FMB5S78BHK6V", instrument: INSTRUMENTS.BTC, side: "sell", qty: "0.06", limit: "50347", stop: "50600", purpose: "protective", tif: "gtc", submitted: t("16:45:20", "2026-09-22"), closed: t("09:44:31", "2026-09-24"), state: "Replaced", note: "Replaced by protection for the whole 0.12." },
  { cid: "cid_01JC353JVN063YY6HMEA1BTNYV", fill: "fil_01JC7RK5R27E63QGJG1284FHTB", instrument: INSTRUMENTS.BTC, side: "buy", qty: "0.06", limit: "56130", stop: null, purpose: "increase", tif: "gtc", submitted: t("09:44:16", "2026-09-24"), closed: t("09:44:20", "2026-09-24"), state: "Filled", at: "56111.1" },
  { cid: "cid_01JC0KDY7FGGXKQR7CNKXFT9XG", instrument: INSTRUMENTS.BTC, side: "buy", qty: "0.01", limit: "55200", stop: null, purpose: "increase", tif: "gtc", submitted: t("10:00:04", "2026-09-26"), closed: t("22:00:00", "2026-09-26"), state: "Canceled", note: "Canceled by the agent: its signal no longer asked for it." },
]);

export const SWING_HISTORY = build([
  { cid: "cid_01JCRVS1ZZZXPZKE0X4VHN2B5T", fill: "fil_01JCK1WGVMPEMVK81DQM2K37BZ", instrument: INSTRUMENTS.XYZ, side: "buy", qty: "4", limit: "138.1", stop: null, purpose: "open", tif: "day", submitted: t("10:05:01", "2026-09-22"), closed: t("10:05:03", "2026-09-22"), state: "Filled", at: "138" },
  { cid: "cid_01JC9JKQZ0BBYPZZ26D5ME88FE", instrument: INSTRUMENTS.XYZ, side: "sell", qty: "4", limit: null, stop: "131.1", purpose: "protective", tif: "gtc", submitted: t("10:05:08", "2026-09-22"), closed: t("09:50:32", "2026-09-23"), state: "Canceled", note: SOLD },
  { cid: "cid_01JCPC1QZH64BTR75N6ZR96K4T", fill: "fil_01JC20BB1ZFT2MHJFK7QV7EZXP", instrument: INSTRUMENTS.XYZ, side: "sell", qty: "4", limit: "147.9", stop: null, purpose: "discretionary_exit", tif: "day", submitted: t("09:50:28", "2026-09-23"), closed: t("09:50:31", "2026-09-23"), state: "Filled", at: "148" },
  { cid: "cid_01JC0BT7MQ95T9TPD11ERZFDZW", fill: "fil_01JCCSC72C14SCSQTCW2GDN2MF", instrument: INSTRUMENTS.QRS, side: "buy", qty: "14", limit: "104.1", stop: null, purpose: "open", tif: "day", submitted: t("10:40:09", "2026-09-23"), closed: t("10:40:12", "2026-09-23"), state: "Filled", at: "104" },
  { cid: "cid_01JCNZQW89Y0C3N9D17NKAK1S1", instrument: INSTRUMENTS.QRS, side: "sell", qty: "14", limit: null, stop: "98.8", purpose: "protective", tif: "gtc", submitted: t("10:40:18", "2026-09-23"), closed: t("10:30:22", "2026-09-24"), state: "Canceled", note: SOLD },
  { cid: "cid_01JCMYQ8TKXMB48WDJS9RXZ5W0", fill: "fil_01JCQVBGWV9C03RKRZXM81QJGM", instrument: INSTRUMENTS.QRS, side: "sell", qty: "14", limit: "98.95", stop: null, purpose: "discretionary_exit", tif: "day", submitted: t("10:30:17", "2026-09-24"), closed: t("10:30:21", "2026-09-24"), state: "Filled", at: "99" },
  { cid: "cid_01JCQ9VEEDH3EVCS5YENX3DR7T", fill: "fil_01JC95AD37XF6XS507PMD8DG8K", instrument: INSTRUMENTS.XYZ, side: "buy", qty: "6", limit: "149.1", stop: null, purpose: "open", tif: "day", submitted: t("11:00:03", "2026-09-24"), closed: t("11:00:06", "2026-09-24"), state: "Filled", at: "149" },
  { cid: "cid_01JCHKG7WZN6896PA9YQTR96MH", instrument: INSTRUMENTS.XYZ, side: "sell", qty: "6", limit: null, stop: "141.55", purpose: "protective", tif: "gtc", submitted: t("11:00:11", "2026-09-24"), closed: t("15:30:45", "2026-09-24"), state: "Canceled", note: SOLD },
  { cid: "cid_01JCHMQEM1YKZF6VQYWT8H3920", fill: "fil_01JCWQM094G6JKJVSXH11NRDD4", instrument: INSTRUMENTS.XYZ, side: "sell", qty: "6", limit: "141.9", stop: null, purpose: "discretionary_exit", tif: "day", submitted: t("15:30:40", "2026-09-24"), closed: t("15:30:44", "2026-09-24"), state: "Filled", at: "141.97" },
  { cid: "cid_01JCNNFPE7KXZ4GPENZFEDEQVH", fill: "fil_01JC39AY2WK08Y82QNM2MN8WF6", instrument: INSTRUMENTS.XYZ, side: "buy", qty: "8", limit: "140.1", stop: null, purpose: "open", tif: "day", submitted: t("10:02:12", "2026-09-25"), closed: t("10:02:15", "2026-09-25"), state: "Filled", at: "140" },
  { cid: "cid_01JC1CSJ8SMSJF3YHBABRNE2MK", instrument: INSTRUMENTS.XYZ, side: "buy", qty: "2", limit: "137.5", stop: null, purpose: "increase", tif: "day", submitted: t("11:00:02", "2026-09-25"), closed: t("16:00:00", "2026-09-25"), state: "Expired", note: "Expired unfilled at the end of the session." },
  { cid: "cid_01JCCAPXG9T236RWCXHTE48HHP", instrument: INSTRUMENTS.QRS, side: "buy", qty: "3", limit: "99.1", stop: null, purpose: "open", tif: "day", submitted: t("13:20:04", "2026-09-25"), closed: t("13:20:05", "2026-09-25"), state: "Rejected", note: "Rejected by the broker: the limit was outside its price band. Nothing was filled." },
  { cid: "cid_01JCGPZ78Y1223KHAFF3AMAPB9", fill: "fil_01JC1WJG5N7EEQE0978A43QKNY", instrument: INSTRUMENTS.QRS, side: "buy", qty: "5", limit: "98.76", stop: null, purpose: "open", tif: "day", submitted: t("10:12:45", "2026-09-28"), closed: t("10:12:47", "2026-09-28"), state: "Filled", at: "98.76" },
]);

export const LMN_HISTORY = build([
  { cid: "cid_01JCNB44SQ54J16GDVJ6VEX5MV", fill: "fil_01JCZXM7MWEZPJ5FKPV1EKVA4H", instrument: INSTRUMENTS.LMN, side: "buy", qty: "20", limit: "44.95", stop: null, purpose: "open", tif: "day", submitted: t("10:15:19", "2026-09-23"), closed: t("10:15:22", "2026-09-23"), state: "Filled", at: "44.92" },
  { cid: "cid_01JCGDWH8PQB4Z9G5QE06EX7ZX", instrument: INSTRUMENTS.LMN, side: "sell", qty: "20", limit: null, stop: "42.67", purpose: "protective", tif: "gtc", submitted: t("10:15:27", "2026-09-23"), closed: t("11:31:03", "2026-09-28"), state: "Canceled", note: SOLD },
  { cid: "cid_01JCNTJP2KTC101AE3F09KCHPN", fill: "fil_01JC5PKDGCCM2YNJZGCX64KMCR", instrument: INSTRUMENTS.LMN, side: "sell", qty: "20", limit: "45.4", stop: null, purpose: "discretionary_exit", tif: "day", submitted: t("11:30:58", "2026-09-28"), closed: t("11:31:02", "2026-09-28"), state: "Filled", at: "45.42" },
]);
