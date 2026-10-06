"use client";

import type { ReactNode } from "react";
import Link from "next/link";
import ReactMarkdown, { type Components, type ExtraProps } from "react-markdown";
import remarkGfm from "remark-gfm";
import { Checkbox } from "pixelarticons/react/Checkbox.js";
import { CheckboxOn } from "pixelarticons/react/CheckboxOn.js";
import { ExternalLink } from "pixelarticons/react/ExternalLink.js";
import { cn } from "@/lib/utils";

type HastNode = NonNullable<ExtraProps["node"]>["children"][number];

function textOf(node: HastNode | NonNullable<ExtraProps["node"]>): string {
  if (node.type === "text") return node.value;
  return "children" in node ? node.children.map(textOf).join("") : "";
}

function hasClass(node: ExtraProps["node"], name: string): boolean {
  const classes = node?.properties.className;
  return Array.isArray(classes) && classes.includes(name);
}

/**
 * Where a link in a message may go: a page in this app, a note on the same message, or an https
 * address. Anything else (`javascript:`, `data:`, `http:`, a protocol-relative `//host`, which a
 * browser also reads from `/\host`) is dropped and the link's words kept as text.
 */
export function safeHref(url: string): string | null {
  const href = url.trim();
  if (/^\/(?![/\\])/.test(href)) return href;
  if (/^#[\w-]+$/.test(href)) return href;
  if (!/^https:\/\//i.test(href)) return null;
  try {
    return new URL(href).protocol === "https:" ? href : null;
  } catch {
    return null;
  }
}

/** Nothing in a message is fetched: an image's address is dropped before it reaches the page. */
function urlTransform(url: string, key: string): string | null {
  return key === "href" ? safeHref(url) : null;
}

const LINK = "rounded-sm font-medium text-lapis underline decoration-lapis/40 underline-offset-2 outline-none hover:decoration-lapis focus-visible:ring-2 focus-visible:ring-ring";

/** What a message's links do: open, or, for text a model wrote, show where they point without opening. */
export type MarkdownLinks = "open" | "show";

function MessageLink({ id, href, children, links, node }: { id?: string; href?: string; children?: ReactNode; links: MarkdownLinks } & ExtraProps) {
  if (!href) return <span data-slot="markdown-link-dropped">{children}</span>;
  if (links === "show") {
    const words = node ? textOf(node) : "";
    return (
      <span data-slot="markdown-link-shown">
        {children}
        {words === href ? null : <span className="font-mono text-[0.9em] text-muted-foreground"> ({href})</span>}
      </span>
    );
  }
  if (href.startsWith("/")) {
    return (
      <Link id={id} href={href} className={LINK}>
        {children}
      </Link>
    );
  }
  if (href.startsWith("#")) {
    return (
      <a id={id} href={href} className={LINK}>
        {children}
      </a>
    );
  }
  return (
    <a id={id} href={href} target="_blank" rel="noopener noreferrer nofollow" className={LINK}>
      {children}
      <ExternalLink aria-hidden className="ml-0.5 inline size-6 align-middle" />
      <span className="sr-only"> (opens in a new tab)</span>
    </a>
  );
}

/** A fenced block: its language named above it, its lines kept as written, scrolling sideways rather than wrapping. */
function CodeBlock({ node }: ExtraProps) {
  const code = node?.children.find((c) => c.type === "element" && c.tagName === "code");
  const classes = code?.type === "element" ? code.properties.className : undefined;
  const language = Array.isArray(classes)
    ? classes
        .map(String)
        .find((c) => c.startsWith("language-"))
        ?.slice("language-".length)
    : undefined;
  const text = code ? textOf(code).replace(/\n$/, "") : "";
  return (
    <figure data-slot="markdown-code" className="grid min-w-0 overflow-hidden rounded-xl border border-border bg-card">
      <figcaption className="border-b border-border/70 px-4 py-1.5 font-mono text-caption text-muted-foreground" translate="no">
        {language ?? "Code"}
      </figcaption>
      <pre tabIndex={0} aria-label={language ? `${language} code` : "Code"} className="overflow-x-auto px-4 py-3 font-mono text-sm leading-relaxed outline-none focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-inset">
        <code translate="no">{text}</code>
      </pre>
    </figure>
  );
}

const PROSE = "max-w-measure text-pretty";

/**
 * A table in a message: framed, scrolling sideways inside its frame when wider than the message
 * (and focusable, so a keyboard can scroll it), its cells never broken mid-word.
 */
export const TABLE = {
  frame: "max-w-full overflow-x-auto rounded-xl border border-border outline-none focus-visible:ring-3 focus-visible:ring-ring",
  table: "w-full border-collapse text-left text-sm wrap-normal tabular",
  head: "bg-foreground/5",
  body: "[&>tr]:border-t [&>tr]:border-border/70",
  th: "px-3 py-2 align-bottom font-semibold whitespace-nowrap",
  td: "px-3 py-2 align-top",
} as const;

/**
 * Headings start at level 4: a message sits under the page's title, the thread's name and its
 * day, and its own headings must not outrank them.
 */
function components(links: MarkdownLinks): Components {
  return {
    h1: ({ id, className, children }) => (
      <h4 id={id} className={cn(PROSE, "text-h3", className)}>
        {children}
      </h4>
    ),
    h2: ({ id, className, children }) => (
      <h5 id={id} className={cn(PROSE, "text-base font-semibold", className)}>
        {children}
      </h5>
    ),
    h3: ({ id, className, children }) => (
      <h6 id={id} className={cn(PROSE, "text-base font-semibold", className)}>
        {children}
      </h6>
    ),
    h4: ({ id, className, children }) => (
      <h6 id={id} className={cn(PROSE, "text-sm font-semibold", className)}>
        {children}
      </h6>
    ),
    h5: ({ id, className, children }) => (
      <h6 id={id} className={cn(PROSE, "text-sm font-semibold", className)}>
        {children}
      </h6>
    ),
    h6: ({ id, className, children }) => (
      <h6 id={id} className={cn(PROSE, "text-sm font-semibold text-muted-foreground", className)}>
        {children}
      </h6>
    ),
    p: ({ children }) => <p className={cn(PROSE, "whitespace-pre-line")}>{children}</p>,
    a: ({ node, id, href, children }) => (
      <MessageLink id={id} href={href} node={node} links={links}>
        {children}
      </MessageLink>
    ),
    ul: ({ node, children }) =>
      hasClass(node, "contains-task-list") ? (
        <ul data-slot="markdown-tasks" className={cn(PROSE, "grid gap-1")}>
          {children}
        </ul>
      ) : (
        <ul className={cn(PROSE, "grid list-outside list-disc gap-1 pl-6 marker:text-muted-foreground")}>{children}</ul>
      ),
    ol: ({ start, children }) => (
      <ol start={start} className={cn(PROSE, "grid list-outside list-decimal gap-1 pl-6 marker:text-muted-foreground")}>
        {children}
      </ol>
    ),
    li: ({ id, children }) => (
      <li id={id} className="pl-1 [&>ol]:mt-1 [&>p+p]:mt-2 [&>ul]:mt-1">
        {children}
      </li>
    ),
    input: ({ type, checked }) =>
      type === "checkbox" ? (
        checked ? (
          <CheckboxOn role="img" aria-label="Done" className="mr-1.5 inline size-6 align-middle" />
        ) : (
          <Checkbox role="img" aria-label="Not done" className="mr-1.5 inline size-6 align-middle text-muted-foreground" />
        )
      ) : null,
    blockquote: ({ children }) => <blockquote className="grid gap-2 border-l-2 border-foreground/20 pl-4 text-muted-foreground">{children}</blockquote>,
    hr: () => <hr className="border-border/70" />,
    pre: ({ node }) => <CodeBlock node={node} />,
    code: ({ children }) => (
      <code className="rounded-md bg-foreground/5 px-1.5 py-0.5 font-mono text-[0.875em]" translate="no">
        {children}
      </code>
    ),
    del: ({ children }) => <del className="text-muted-foreground">{children}</del>,
    table: ({ children }) => (
      <div data-slot="markdown-table" tabIndex={0} role="region" aria-label="Table" className={TABLE.frame}>
        <table className={TABLE.table}>{children}</table>
      </div>
    ),
    thead: ({ children }) => <thead className={TABLE.head}>{children}</thead>,
    tbody: ({ children }) => <tbody className={TABLE.body}>{children}</tbody>,
    th: ({ style, children }) => (
      <th scope="col" style={style} className={TABLE.th}>
        {children}
      </th>
    ),
    td: ({ style, children }) => (
      <td style={style} className={TABLE.td}>
        {children}
      </td>
    ),
    img: ({ alt }) => (
      <span data-slot="markdown-image" className="text-muted-foreground">
        {alt ? `[Image not shown: ${alt}]` : "[Image not shown]"}
      </span>
    ),
  };
}

const PLUGINS = [remarkGfm];
const OPEN = components("open");
const SHOW = components("show");

/**
 * A message's text as GitHub-flavoured Markdown: headings, lists and task lists, tables that scroll
 * sideways, fenced code, quotes, strikethrough, and links. It renders no HTML (raw tags read as the
 * text they are), fetches nothing (an image shows as its alt text), and opens only an in-app page or
 * an https address, in a new tab with no referrer. A single line break is kept, as typed.
 */
export function Markdown({ text, links = "open", className }: { text: string; links?: MarkdownLinks; className?: string }) {
  return (
    <div data-slot="markdown" className={cn("grid min-w-0 gap-3 wrap-break-word", className)}>
      <ReactMarkdown remarkPlugins={PLUGINS} urlTransform={urlTransform} components={links === "open" ? OPEN : SHOW}>
        {text}
      </ReactMarkdown>
    </div>
  );
}
