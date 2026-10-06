import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Markdown, safeHref } from "./markdown";

function md(text: string, links: "open" | "show" = "open") {
  const { container } = render(<Markdown text={text} links={links} />);
  return container.querySelector<HTMLElement>("[data-slot=markdown]")!;
}

describe("a message's Markdown", () => {
  it("draws a GFM table in a frame that scrolls sideways, with header cells and the column's alignment", () => {
    const box = md("| Limit | Headroom |\n| :-- | --: |\n| Daily loss | $120.00 |\n| Position | $88.00 |");
    const frame = box.querySelector<HTMLElement>("[data-slot=markdown-table]")!;
    expect(frame).toHaveAttribute("tabindex", "0");
    expect(frame.className).toContain("overflow-x-auto");
    const table = within(frame).getByRole("table");
    expect(within(table).getAllByRole("columnheader").map((h) => h.textContent)).toEqual(["Limit", "Headroom"]);
    expect(within(table).getAllByRole("row")).toHaveLength(3);
    expect(within(table).getByRole("cell", { name: "$120.00" })).toHaveStyle({ textAlign: "right" });
  });

  it("names a fenced block's language and keeps its lines as written", () => {
    const box = md("```ts\nconst limit = 1_000;\n  return limit;\n```");
    const block = box.querySelector<HTMLElement>("[data-slot=markdown-code]")!;
    expect(block.querySelector("figcaption")).toHaveTextContent("ts");
    const pre = block.querySelector("pre")!;
    expect(pre).toHaveAttribute("tabindex", "0");
    expect(pre.className).toContain("overflow-x-auto");
    expect(pre.textContent).toBe("const limit = 1_000;\n  return limit;");
  });

  it("draws inline code, emphasis, strikethrough, quotes, rules and lists", () => {
    const box = md("Use `pause` **now**, _maybe_ ~~later~~.\n\n> quoted\n\n---\n\n1. one\n2. two\n\n- a\n- b");
    expect(box.querySelector("code")).toHaveTextContent("pause");
    expect(box.querySelector("strong")).toHaveTextContent("now");
    expect(box.querySelector("em")).toHaveTextContent("maybe");
    expect(box.querySelector("del")).toHaveTextContent("later");
    expect(box.querySelector("blockquote")).toHaveTextContent("quoted");
    expect(box.querySelector("hr")).not.toBeNull();
    expect([...box.querySelectorAll("ol > li")].map((li) => li.textContent)).toEqual(["one", "two"]);
    expect([...box.querySelectorAll("ul > li")].map((li) => li.textContent)).toEqual(["a", "b"]);
  });

  it("draws a task list as done and not done, with nothing to tick", () => {
    const box = md("- [x] set the cap\n- [ ] confirm");
    expect(box.querySelector("input")).toBeNull();
    expect(within(box).getByRole("img", { name: "Done" })).toBeInTheDocument();
    expect(within(box).getByRole("img", { name: "Not done" })).toBeInTheDocument();
  });

  it("keeps a single line break as typed", () => {
    const box = md("first line\nsecond line");
    const p = box.querySelector("p")!;
    expect(p.textContent).toBe("first line\nsecond line");
    expect(p.className).toContain("whitespace-pre-line");
  });

  it("starts headings at level 4, under the page, the thread and its day", () => {
    const box = md("# One\n\n## Two\n\n### Three");
    expect(within(box).getByRole("heading", { name: "One" }).tagName).toBe("H4");
    expect(within(box).getByRole("heading", { name: "Two" }).tagName).toBe("H5");
    expect(within(box).getByRole("heading", { name: "Three" }).tagName).toBe("H6");
  });

  it("renders no HTML: tags read as the text they are", () => {
    const box = md('<b>bold</b> <img src="https://example.com/x.png" onerror="alert(1)"> <script>alert(1)</script>');
    expect(box.querySelector("b, script, iframe")).toBeNull();
    expect(box.querySelector("img")).toBeNull();
    expect(box).toHaveTextContent("<b>bold</b>");
    expect(box).toHaveTextContent("<script>alert(1)</script>");
  });

  it("fetches no image: it shows as its alt text", () => {
    const box = md("![a chart of fills](https://example.com/track.png)");
    expect(box.querySelector("img")).toBeNull();
    expect(box.querySelector("[data-slot=markdown-image]")).toHaveTextContent("[Image not shown: a chart of fills]");
    expect(box.innerHTML).not.toContain("example.com");
  });

  it("opens an https link in a new tab with no referrer, and an app page in place", () => {
    const box = md("[Docs](https://example.com/docs) and [the request](/approvals/apr_1) and https://example.org");
    const docs = within(box).getByRole("link", { name: /^Docs/ });
    expect(docs).toHaveAttribute("href", "https://example.com/docs");
    expect(docs).toHaveAttribute("target", "_blank");
    expect(docs).toHaveAttribute("rel", "noopener noreferrer nofollow");
    expect(docs).toHaveTextContent("(opens in a new tab)");
    const request = within(box).getByRole("link", { name: "the request" });
    expect(request).toHaveAttribute("href", "/approvals/apr_1");
    expect(request).not.toHaveAttribute("target");
    expect(within(box).getByRole("link", { name: /^https:\/\/example\.org/ })).toHaveAttribute("href", "https://example.org");
  });

  it.each(["javascript:alert(1)", "JaVaScRiPt:alert(1)", "data:text/html,<b>x</b>", "http://example.com", "//evil.example", "vbscript:x", "mailto:a@b.c"])(
    "drops a link to %s and keeps its words",
    (href) => {
      const box = md(`[click me](${href})`);
      expect(within(box).queryByRole("link")).toBeNull();
      expect(box).toHaveTextContent("click me");
    },
  );

  it("keeps a backslash after the slash percent-encoded, so the link stays on this app", () => {
    const box = md("[click me](/\\evil.example)");
    expect(within(box).getByRole("link", { name: "click me" }).getAttribute("href")).toBe("/%5Cevil.example");
  });

  it("shows where a model's links point, without opening them", () => {
    const box = md("See [the source](https://example.com/s) or https://example.org", "show");
    expect(within(box).queryByRole("link")).toBeNull();
    expect(box).toHaveTextContent("See the source (https://example.com/s) or https://example.org");
    expect(box).not.toHaveTextContent("https://example.org (https://example.org)");
  });

  it("never draws a button, whatever the text says", () => {
    const box = md("<button>Approve</button>\n\n[Approve](javascript:approve())\n\n- [ ] Approve");
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByRole("checkbox")).toBeNull();
    expect(within(box).queryByRole("link")).toBeNull();
  });
});

describe("safeHref", () => {
  it.each([
    ["/agents/agt_1", "/agents/agt_1"],
    ["#user-content-fn-1", "#user-content-fn-1"],
    ["https://example.com/a?b=c#d", "https://example.com/a?b=c#d"],
    [" https://example.com ", "https://example.com"],
  ])("keeps %s", (href, kept) => {
    expect(safeHref(href)).toBe(kept);
  });

  it.each(["javascript:alert(1)", "http://example.com", "//example.com", "/\\example.com", "ftp://example.com", "https:example.com", "#a b", ""])("drops %s", (href) => {
    expect(safeHref(href)).toBeNull();
  });
});
