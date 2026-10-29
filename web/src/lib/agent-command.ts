/**
 * What the owner types to one of their agents, read as one of a fixed set of actions (the founder:
 * "type what I want in simple words, and that does things"). It is a plain parser, with no model: each
 * phrase names exactly one action, which the screen shows as a one-line preview and sends only when
 * the owner taps Send. It never advises: a question about what to trade is refused, and a buy is the
 * owner's own request to the agent, which its builder sizes within the mandate (DEC-2151, DEC-528).
 */

export type AgentCommand =
  /** "buy 10 AAPL": the owner asks the agent to buy `size` of `symbol` (POST /agents/{id}/requests). */
  | { kind: "ask_buy"; size: string; symbol: string }
  | { kind: "pause" }
  | { kind: "resume" }
  | { kind: "stop" }
  /** The agent-scoped kill switch: cancel its orders and sell what it holds (rule 13). */
  | { kind: "kill" }
  /** "exit AAPL" sells that one; "sell everything" or a bare "exit" sells every holding (owner exits). */
  | { kind: "exit"; symbol: string | null }
  /** "how am I doing", "positions": an answer from the reads, which sends nothing. */
  | { kind: "status" }
  /** "should I buy…": the app gives no trade advice. */
  | { kind: "advice" }
  | { kind: "unknown" };

/** What the screen offers when it cannot tell what was meant. */
export const EXAMPLES = ["buy 10 AAPL", "how am I doing", "pause", "resume", "sell everything", "exit AAPL", "stop", "kill switch"] as const;

const SYMBOL = "([a-z][a-z0-9.]{0,9}(?:/[a-z]{2,5})?)";
const SIZE = "((?:0|[1-9][0-9]*)(?:\\.[0-9]+)?)";
const POLITE = "(?:please\\s+)?(?:(?:can|could|would) you\\s+)?(?:(?:ask (?:it|the agent|my agent) to|tell (?:it|the agent) to|i want to|i'd like to|let's)\\s+)?";

const BUY = new RegExp(`^${POLITE}(?:buy|purchase)\\s+${SIZE}\\s*(?:shares?\\s+(?:of\\s+)?|units?\\s+(?:of\\s+)?)?\\$?${SYMBOL}$`);
const BUY_SYMBOL_FIRST = new RegExp(`^${POLITE}(?:buy|purchase)\\s+${SYMBOL}\\s+x?\\s*${SIZE}$`);
const EXIT_ONE = new RegExp(`^${POLITE}(?:exit|sell all(?: of)?|sell|close)\\s+(?:my\\s+)?${SYMBOL}(?:\\s+position)?$`);
const EXIT_ALL = new RegExp(`^${POLITE}(?:exit|sell everything|sell it all|sell all|close everything|close all|flatten|exit everything|exit all)(?:\\s+positions?)?$`);
const ADVICE = /\b(should i|should it|is it a good|good (time|idea)|recommend|what (should|to) (i )?buy|worth buying|advi[cs]e)\b/;
const STATUS = /^(how am i doing|how('s| is) it (doing|going)|how are we doing|positions?|my positions|what do i (hold|own)|what does it (hold|own)|status|holdings|p&l|pnl|performance)$/;

function plain(said: string): string {
  return said
    .toLowerCase()
    .replace(/[?!.,]+$/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

/** The action `said` names, or `unknown`. A buy's symbol comes back upper-case, as `GET /instruments` writes it. */
export function readCommand(said: string): AgentCommand {
  const text = plain(said);
  if (text === "") return { kind: "unknown" };
  if (ADVICE.test(text)) return { kind: "advice" };
  const buy = BUY.exec(text);
  if (buy) return { kind: "ask_buy", size: buy[1], symbol: buy[2].toUpperCase() };
  const buyFirst = BUY_SYMBOL_FIRST.exec(text);
  if (buyFirst) return { kind: "ask_buy", size: buyFirst[2], symbol: buyFirst[1].toUpperCase() };
  if (STATUS.test(text)) return { kind: "status" };
  if (/^(?:please\s+)?(?:kill switch|kill it|kill the agent|emergency stop|use the kill switch)$/.test(text)) return { kind: "kill" };
  if (EXIT_ALL.test(text)) return { kind: "exit", symbol: null };
  const exitOne = EXIT_ONE.exec(text);
  if (exitOne && !["everything", "all", "it"].includes(exitOne[1])) return { kind: "exit", symbol: exitOne[1].toUpperCase() };
  if (/^(?:please\s+)?(?:pause|pause it|pause the agent|hold off|freeze)$/.test(text)) return { kind: "pause" };
  if (/^(?:please\s+)?(?:resume|resume it|unpause|start again|carry on)$/.test(text)) return { kind: "resume" };
  if (/^(?:please\s+)?(?:stop|stop it|stop the agent|retire it|retire the agent)$/.test(text)) return { kind: "stop" };
  return { kind: "unknown" };
}
