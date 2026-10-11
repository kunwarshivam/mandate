/*
 * The tour's frames, drawn in the browser by `render-tour.ts`: `window.render(t)` paints the frame
 * at t seconds. Everything is a pure function of t, so a frame renders the same every time.
 * `window.TOUR_DATA` carries the scenes and captions (tour.ts), the cue times, the typed rules and
 * the frame rate (tour-cues.ts), the owls' pixels (owl-sprite.ts) and the wallpapers.
 */
(() => {
  const { TOUR, CUES: C, RULES_TEXT, WIPE, FPS, CAST, DAY, NIGHT } = window.TOUR_DATA;

  const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
  const span = (t, a, d) => clamp((t - a) / d, 0, 1);
  const out = (x) => 1 - Math.pow(1 - clamp(x, 0, 1), 3);
  const back = (x) => {
    const k = clamp(x, 0, 1) - 1;
    return 1 + 2.9 * k * k * k + 1.9 * k * k;
  };
  const lerp = (a, b, k) => a + (b - a) * k;
  const noise = (n) => {
    const x = Math.sin(n * 12.9898 + 78.233) * 43758.5453;
    return x - Math.floor(x);
  };
  const frame = (t) => Math.floor(t * FPS + 1e-6);

  /** The product's owl, pixel for pixel: blinking on its own rhythm, pupils a whole pixel toward `gaze`. */
  function owl(o, size, { t = 0, mood = "awake", gaze = [0, 0], style = "" } = {}) {
    const eyes = o.eyes[mood];
    const phase = ((((t + o.blink.offset) % o.blink.every) + o.blink.every) % o.blink.every) / o.blink.every;
    const blinking = phase > 0.935 && phase < 0.975;
    const gx = Math.round(clamp(gaze[0], -1, 1));
    const gy = Math.round(clamp(gaze[1], -1, 1));
    const rect = (q, dx = 0, dy = 0) => `<rect x="${(q.x + dx) * 4}" y="${(q.y + dy) * 4}" width="${q.w * 4}" height="${q.h * 4}" fill="${q.fill}"${q.opacity == null ? "" : ` opacity="${q.opacity}"`}/>`;
    const open = blinking ? "" : eyes.open.flat().map((q) => rect(q, gx, gy)).join("");
    return `<svg class="owl" width="${size}" height="${size}" viewBox="0 0 64 64" shape-rendering="crispEdges" style="${style}">${o.body.map((q) => rect(q)).join("")}${open}${eyes.fixed.map((q) => rect(q)).join("")}</svg>`;
  }

  /** Where an owl at (x, y) looks to see (tx, ty). */
  const look = (x, y, tx, ty) => {
    const d = Math.hypot(tx - x, ty - y) || 1;
    return [((tx - x) / d) * 1.2, ((ty - y) / d) * 1.2];
  };

  /** A pop: grows past its size and settles, fading in on the way. */
  const pop = (t, at, d = 0.36) => {
    const k = span(t, at, d);
    return { k, style: `transform:scale(${lerp(0.55, 1, back(k))});opacity:${clamp(k * 4, 0, 1)}` };
  };

  /** A decaying shake from `at`, whole pixels only. */
  function shake(t, at, d, amp, seed) {
    if (t < at || t > at + d) return [0, 0];
    const left = 1 - (t - at) / d;
    const f = frame(t);
    return [Math.round((noise(f + seed) - 0.5) * 2 * amp * left), Math.round((noise(f * 7 + seed) - 0.5) * 2 * amp * left)];
  }

  /** The camera between keyframes [t, zoom, focus x, focus y], easing out into each. */
  function camera(t, keys) {
    let i = 0;
    while (i < keys.length - 1 && t >= keys[i + 1][0]) i++;
    const a = keys[i];
    const b = keys[Math.min(i + 1, keys.length - 1)];
    const k = b === a ? 0 : out(span(t, a[0], b[0] - a[0]));
    return { z: lerp(a[1], b[1], k), fx: lerp(a[2], b[2], k), fy: lerp(a[3], b[3], k) };
  }

  const cam = (inner, { z, fx, fy }, [sx, sy] = [0, 0]) => `<div class="cam" style="transform-origin:${fx}px ${fy}px;transform:translate(${sx}px,${sy}px) scale(${z})">${inner}</div>`;
  const flash = (t, at, d, colour, peak) => (t >= at && t < at + d ? `<div class="stage" style="background:${colour};opacity:${peak * (1 - (t - at) / d)}"></div>` : "");
  const cursor = (x, y, down) =>
    `<svg class="cursor" style="left:${x}px;top:${y}px;transform:scale(${down ? 0.86 : 1})" viewBox="0 0 11 16" shape-rendering="crispEdges"><path d="M0 0v14l3-3 2 5 2-1-2-5h4z" fill="${down ? "var(--highlight)" : "var(--card)"}" stroke="var(--foreground)" stroke-width="1"/></svg>`;
  const glide = (t, [a, b], from, to) => {
    const k = out(span(t, a, b - a));
    return [lerp(from[0], to[0], k), lerp(from[1], to[1], k)];
  };
  const win = (x, y, w, h, title, body, style = "") =>
    `<div class="win raised" style="left:${x}px;top:${y}px;width:${w}px;height:${h}px;transform-origin:center;${style}"><div class="title">${owl(CAST.brand, 22)}${title}<span class="x"><span class="raised"></span><span class="raised"></span><span class="raised"></span></span></div><div class="body sunken">${body}</div></div>`;
  const desk = (img, inner, clock) =>
    `<div class="wall" style="background-image:url(${img})"></div>${inner}<div class="taskbar"><span class="start raised">${owl(CAST.brand, 26)} Start</span><span class="clock sunken">${clock}</span></div>`;
  const typed = (text, n) => text.slice(0, Math.max(0, Math.floor(n)));
  const caret = (t) => (Math.floor(t * 2) % 2 ? "&nbsp;" : "▌");
  const dots = (t) => ".".repeat(1 + (Math.floor(t * 3) % 3));

  const SCENES = {
    boot(t) {
      const c = C.boot;
      const fall = span(t, c.drop, c.land - c.drop);
      const y = t < c.land ? lerp(-520, 0, fall * fall) : 0;
      const squash = t >= c.land && t < c.land + 0.24 ? Math.sin(span(t, c.land, 0.24) * Math.PI) : 0;
      const hop = t >= c.jingle ? Math.sin(span(t, c.jingle, 0.36) * Math.PI) * 26 : 0;
      const gaze = t < c.land ? [0, -1] : t < c.bar[0] ? [0, 1] : t < c.jingle ? [Math.round(lerp(-1, 1, span(t, c.bar[0], c.bar[1] - c.bar[0]))), 1] : [0, 0];
      const word = [..."Owlhead 98"]
        .map((ch, i) => {
          const k = span(t, c.letters + i * c.letterGap, 0.32);
          return `<span style="display:inline-block;transform:translateY(${(1 - back(k)) * 46}px) scale(${lerp(0.3, 1, back(k))});opacity:${clamp(k * 4, 0, 1)};${i >= 8 ? "color:var(--highlight)" : ""}">${ch === " " ? "&nbsp;" : ch}</span>`;
        })
        .join("");
      const bars = Math.floor(span(t, c.bar[0], c.bar[1] - c.bar[0]) * c.bars);
      const ready = t >= c.jingle;
      const shook = shake(t, c.land, 0.3, 10, 1);
      const inner =
        `<div class="stage" style="display:grid;place-content:center;justify-items:center;gap:20px;padding-bottom:150px;background:var(--ink);color:var(--ink-foreground)">` +
        owl(CAST.brand, 208, { t, gaze, style: `transform:translateY(${y - hop}px) scale(${1 + squash * 0.14},${1 - squash * 0.14});transform-origin:bottom` }) +
        `<div class="pix" style="font-size:76px;font-weight:600;line-height:1;height:78px">${word}</div>` +
        `<div class="sunken" style="width:420px;height:30px;padding:3px;display:flex;gap:3px;background:var(--ink)">${Array.from({ length: bars }, () => `<i style="width:calc((100% - ${3 * (c.bars - 1)}px) / ${c.bars});background:var(--highlight)"></i>`).join("")}</div>` +
        `<div class="mono" style="font-size:30px;color:${ready ? "var(--highlight)" : "var(--series-5)"}">${ready ? "Ready." : `Starting Owlhead${dots(t)}`}</div></div>`;
      return cam(inner, camera(t, [[0, 1, 640, 360], [c.jingle, 1.04, 640, 330], [5, 1.08, 640, 330]]), shook);
    },

    rules(t) {
      const c = C.rules;
      const shown = typed(RULES_TEXT, (t - c.typeFrom) * c.typeRate).replace(/\n/g, "<br>");
      const note = t >= c.win ? win(80, 56, 610, 300, "mandate.txt - Notepad", `<div class="mono" style="font-size:32px;line-height:1.3">${shown}${caret(t)}</div>`, pop(t, c.win).style) : "";
      let dialog = "";
      if (t >= c.dialog) {
        const pressed = t >= c.press;
        const down = t >= c.press && t < c.press + 0.16;
        const [cx, cy] = glide(t, c.cursor, [1180, 650], [1074, 428]);
        dialog =
          win(
            700,
            140,
            500,
            350,
            "Owlhead understood",
            `<div style="display:flex;gap:14px;align-items:center;padding-bottom:6px">${owl(CAST.brand, 56, { t, gaze: t < c.cursor[0] ? [-1, 0] : look(1150, 200, cx, cy) })}<div class="pix" style="font-size:21px">Here are your rules, in dollars:</div></div>` +
              '<div class="row"><span>Can buy</span><span>US stocks</span></div><div class="row"><span>Largest order</span><span>$2,000</span></div><div class="row"><span>Asks you before</span><span>anything new to you</span></div>' +
              `<div style="display:flex;gap:10px;justify-content:flex-end;padding-top:16px"><span class="btn ${pressed ? "sunken lit" : "raised go"}" style="transform:scale(${down ? 0.94 : 1})">${pressed ? "Confirmed" : "Confirm"}</span><span class="btn raised">Edit</span></div>`,
            pop(t, c.dialog).style,
          ) + cursor(cx, cy, down);
      }
      const view = camera(t, [[0, 1.17, 385, 205], [c.dialog, 1.17, 385, 205], [c.dialog + 0.6, 1, 640, 360], [c.press - 0.15, 1, 640, 360], [c.press + 0.15, 1.15, 1000, 440], [c.press + 1.1, 1.15, 1000, 440], [10, 1.04, 820, 330]]);
      return cam(desk(DAY, note + dialog + flash(t, c.press, 0.25, "var(--highlight)", 0.22), "9:29 AM"), view, shake(t, c.press, 0.18, 4, 2));
    },

    ideas(t) {
      const c = C.ideas;
      const lines = [
        ["Idea 14", "Buy XYZ"],
        ["Why", "Orders have grown three quarters running."],
        ["Wrong if", "Next quarter’s orders fall."],
        ["Exit plan", "Stop $84 · target $104 · 30 days"],
      ];
      const landed = lines.map((_, i) => c.row0 + i * c.rowGap);
      const rows = lines
        .filter((_, i) => t >= landed[i])
        .map(([a, b], i) => {
          const k = span(t, landed[i], 0.4);
          const glow = 1 - span(t, landed[i] + 0.1, 0.7);
          return `<div class="row" style="transform:translateX(${(1 - back(k)) * 90}px);opacity:${clamp(k * 3, 0, 1)};background:color-mix(in oklab, var(--highlight) ${Math.round(glow * 70)}%, transparent)"><span>${a}</span><span>${b}</span></div>`;
        })
        .join("");
      const last = landed.filter((at) => t >= at).at(-1) ?? -9;
      const hop = Math.sin(span(t, last, 0.3) * Math.PI) * 14;
      const scan = [-1, 0, 1, 0][Math.floor(t * 2.6) % 4];
      const gaze = t - last < 0.6 ? [1, 0] : [scan, 1];
      const reading = `<div class="mono" style="font-size:26px;color:var(--muted-foreground);padding-bottom:8px">Reading filings, news and prices${dots(t)}</div>`;
      const body = `<div style="display:flex;gap:28px"><div style="padding-top:10px">${owl(CAST.perch[0], 150, { t, gaze, style: `transform:translateY(${-hop}px)` })}</div><div style="flex:1">${reading}${rows}</div></div>`;
      const shown = t >= c.win ? win(160, 64, 960, 320, "ideas.txt - Owlhead", body, pop(t, c.win).style) : "";
      return cam(desk(DAY, shown, "9:31 AM"), camera(t, [[0, 1, 640, 300], [c.row0, 1, 640, 300], [c.row0 + 0.5, 1.07, 700, 240], [8, 1.07, 700, 240]]));
    },

    orders(t) {
      const c = C.orders;
      const steps = ["Idea 14: buy XYZ", "Sized: 20 XYZ for $1,840", "Checked: under $2,000, a US stock", "New to you, so it asks first"];
      const lit = steps.map((_, i) => c.step0 + i * c.stepGap);
      const flow =
        '<div style="position:absolute;left:70px;top:52px;display:grid">' +
        steps
          .map((s, i) => {
            const on = t >= lit[i];
            const bump = 1 + 0.08 * Math.sin(span(t, lit[i], 0.26) * Math.PI);
            const tick = i > 0 && i < 3 && on ? `<span class="good" style="display:inline-block;${pop(t, lit[i] + 0.08, 0.3).style}">✓</span>` : "";
            return (i ? '<div class="arrow">↓</div>' : "") + `<div class="box ${on ? "raised lit" : "raised"}" style="transform:scale(${bump})"><span class="pix">${s} ${tick}</span></div>`;
          })
          .join("") +
        "</div>";
      const step = lit.filter((at) => t >= at).length - 1;
      const asking = t >= c.phone;
      const watcher = owl(CAST.perch[0], 112, { t, gaze: asking ? [1, 0] : step < 0 ? [0, 0] : look(590, 120, 290, 90 + step * 116), style: "position:absolute;left:560px;top:70px" });
      const approved = t >= c.approved;
      const card =
        '<div style="background:var(--card);padding:14px;font-size:19px;line-height:1.3">' +
        (approved
          ? `<div style="${pop(t, c.approved, 0.3).style}"><div class="pix good" style="font-size:26px;font-weight:600">Approved.</div><div>Order sent to your broker. It’s in the record.</div></div>`
          : `<div class="pix" style="font-weight:600;font-size:21px;padding-bottom:6px">Owlhead wants to buy</div><div style="font-size:24px">20 XYZ · $1,840</div><div style="color:var(--muted-foreground);padding:6px 0 12px">Your rules say to ask before buying anything you haven’t held.</div><div style="display:flex;gap:8px"><span class="btn ${t >= c.press ? "sunken lit" : "raised go"}">Approve</span><span class="btn raised">Decline</span></div>`) +
        "</div>";
      const rise = back(span(t, c.phone, 0.5));
      const buzz = t >= c.buzz && t < c.buzz + 0.45 && Math.floor((t - c.buzz) / 0.15) % 2 === 0 ? Math.round(Math.sin(frame(t) * 2.4) * 5) : 0;
      const phone = asking
        ? `<div style="position:absolute;left:${760 + buzz}px;top:${lerp(720, 40, rise)}px;width:330px;height:560px;background:var(--ink);border-radius:34px;padding:52px 18px"><div class="mono" style="color:var(--ink-foreground);font-size:22px;text-align:center;padding-bottom:12px">9:32</div>${card}</div>`
        : "";
      const down = t >= c.press && t < c.press + 0.16;
      const tap = t >= c.cursor[0] ? cursor(...glide(t, c.cursor, [1150, 650], [896, 330]), down) : "";
      const view = camera(t, [[0, 1, 640, 360], [c.phone + 0.2, 1, 640, 360], [c.phone + 0.75, 1.22, 925, 330], [c.approved + 0.9, 1.22, 925, 330], [c.approved + 1.5, 1, 640, 360]]);
      return cam(desk(DAY, flow + watcher + phone + tap + flash(t, c.approved, 0.3, "var(--highlight)", 0.25), "9:32 AM"), view);
    },

    record(t) {
      const c = C.record;
      const rows = [
        ["09:30:58", "read", "XYZ quarterly report", "a41f9b"],
        ["09:31:02", "idea", "Idea 14: buy XYZ", "7c02e1"],
        ["09:31:03", "sized", "20 XYZ, $1,840", "3f9a44"],
        ["09:31:03", "checked", "within your rules", "e5b710"],
        ["09:31:04", "asked", "the owner, new to you", "9d2c8f"],
        ["09:32:10", "approved", "by the owner, on phone", "b06e53"],
        ["09:32:11", "sent", "to Alpaca, on paper", "4c8ad2"],
      ];
      const head = ["TIME", "WHAT", "DETAIL", "HASH"].map((h) => `<span class="pix" style="font-size:17px;color:var(--muted-foreground)">${h}</span>`).join("");
      const glitching = t >= c.tamper && t < c.tamper + 0.3;
      const body =
        `<div class="rec">${head}` +
        rows
          .map((r, i) => {
            const at = c.row0 + i * c.rowGap;
            if (t < at) return "";
            const breaks = c.tamper + 0.15 + (i - 2) * c.cascadeGap;
            const broken = i >= 2 && t >= breaks;
            const settling = t < at + 0.32;
            const hash = settling ? [...r[3]].map((_, j) => "0123456789abcdef"[Math.floor(noise(frame(t) * 13 + i * 7 + j) * 16)]).join("") : r[3];
            const tampered = i === 2 && t >= c.tamper;
            const jitter = tampered && glitching ? Math.round((noise(frame(t) + 3) - 0.5) * 18) : 0;
            const detail = tampered
              ? `<span class="bad" style="display:inline-block;transform:translateX(${jitter}px);background:color-mix(in oklab,var(--crimson) 16%,transparent)">${glitching && frame(t) % 2 ? r[2] : "200 XYZ, $18,400"}</span>`
              : r[2];
            const mark = broken ? `<span class="bad" style="display:inline-block;${pop(t, breaks, 0.28).style}">✗ no match</span>` : `<span class="${settling ? "" : "good"}">${settling ? "" : "✓ "}${hash}</span>`;
            const k = span(t, at, 0.22);
            return `<span style="opacity:${k}">${r[0]}</span><span style="opacity:${k}">${r[1]}</span><span style="opacity:${k}">${detail}</span>${mark}`;
          })
          .join("") +
        "</div>";
      const shown = t >= c.win ? win(110, 50, 1060, 370, "The record - Owlhead", body, pop(t, c.win).style) : "";
      const down = t >= c.click && t < c.click + 0.15;
      const edit = t >= c.cursor[0] && t < c.tamper + 1.2 ? cursor(...glide(t, c.cursor, [760, 620], [402, 214]), down) : "";
      const view = camera(t, [[0, 1, 640, 235], [c.cursor[1], 1, 640, 235], [c.tamper + 0.15, 1.18, 520, 250], [c.tamper + 2.4, 1.18, 520, 250], [c.tamper + 2.9, 1.04, 640, 250]]);
      return cam(desk(DAY, shown + edit + flash(t, c.tamper, 0.22, "var(--crimson)", 0.2), "9:33 AM"), view, shake(t, c.tamper, 0.5, 12, 3));
    },

    check(t) {
      const c = C.check;
      const items = ["Paper first, with simulated money.", "One Stop button halts every agent.", "It can’t take money out of your account."];
      const halted = t >= c.press;
      const down = t >= c.press && t < c.press + 0.18;
      const [cx, cy] = t >= c.cursor[0] ? glide(t, c.cursor, [1100, 620], [288, 352]) : [1100, 620];
      // Every line is laid out from the start, unseen until its cue, so Stop never moves under the cursor.
      const list = items
        .map((s, i) => {
          const at = c.item0 + i * c.itemGap;
          const k = span(t, at, 0.36);
          return `<div style="display:flex;gap:14px;align-items:center;font-size:27px;padding:10px 0;transform:translateX(${(1 - back(k)) * -60}px);opacity:${clamp(k * 3, 0, 1)}"><span class="good pix" style="font-size:30px;display:inline-block;${pop(t, at + 0.1, 0.3).style}">✓</span>${s}</div>`;
        })
        .join("");
      const stop =
        t >= c.stop
          ? `<div style="display:flex;align-items:center;gap:22px;padding-top:12px"><span class="stop ${down ? "sunken" : "raised"}" style="display:inline-block;${pop(t, c.stop).style}">STOP</span><span class="mono" style="font-size:26px;color:var(--muted-foreground)">${halted ? "3 agents halted · 2 open orders cancelled" : "3 agents trading"}</span></div>`
          : "";
      const crew = [CAST.perch[0], CAST.perch[1], CAST.perch[3]];
      const owls =
        '<div style="display:flex;gap:18px;position:absolute;right:30px;top:24px">' +
        crew
          .map((o, i) => {
            const drop = halted ? out(span(t, c.press + i * 0.125, 0.2)) * 8 : 0;
            const x = 820 + i * 114;
            return owl(o, 96, { t: t + i, mood: t >= c.press + i * 0.125 ? "stopped" : "awake", gaze: t >= c.cursor[0] ? look(x, 160, cx, cy) : [0, 1], style: `transform:translateY(${drop}px)` });
          })
          .join("") +
        "</div>";
      const shown = t >= c.win ? win(150, 66, 980, 340, "How it stays in check", `<div style="position:relative">${owls}${list}${stop}</div>`, pop(t, c.win).style) : "";
      const pointer = t >= c.cursor[0] && t < c.press + 2 ? cursor(cx, cy, down) : "";
      const view = camera(t, [[0, 1, 640, 300], [c.press - 0.1, 1, 640, 300], [c.press + 0.12, 1.16, 330, 330], [c.press + 2.2, 1.16, 330, 330], [c.press + 2.9, 1.02, 640, 300]]);
      return cam(desk(DAY, shown + pointer + flash(t, c.press, 0.18, "var(--card)", 0.5), "9:34 AM"), view, shake(t, c.press, 0.55, 16, 4));
    },

    end(t) {
      const c = C.end;
      const fade = span(t, c.fade, 1);
      const hop = Math.sin(span(t, c.sting, 0.32) * Math.PI) * 18 + Math.sin(span(t, c.sting + 0.36, 0.28) * Math.PI) * 10;
      const perch = CAST.perch
        .map((o, i) => {
          const at = c.perch0 + i * c.perchGap;
          return t < at ? '<span style="width:60px"></span>' : owl(o, 60, { t: t + i * 0.7, mood: o.mood, gaze: [0, 0], style: pop(t, at, 0.34).style });
        })
        .join("");
      const pulse = t > 2.6 ? 1 + 0.035 * Math.sin((t - 2.6) * 4.2) : 1;
      const body =
        '<div style="display:grid;justify-items:center;gap:14px;padding-top:6px;text-align:center">' +
        `<div style="display:flex;align-items:center;gap:20px">${owl(CAST.brand, 112, { t, gaze: [0, 0], style: `transform:translateY(${-hop}px)` })}<div class="pix" style="font-size:64px;font-weight:600;line-height:1">Owlhead</div></div>` +
        '<div style="font-size:24px;max-width:580px">A trading agent for your own brokerage account. It works inside rules you write, and it writes down every decision it makes.</div>' +
        `<div style="display:grid;justify-items:center"><div style="display:flex;gap:16px;align-items:flex-end;height:62px">${perch}</div><div style="height:6px;width:420px;background:var(--foreground)"></div></div>` +
        `<div class="btn raised go" style="font-size:22px;transform:scale(${pulse})">Ask for a place at owlhead.ai</div></div>`;
      const shown = t >= c.win ? win(270, 26, 740, 480, "Owlhead", body, pop(t, c.win, 0.42).style) : "";
      return cam(desk(NIGHT, shown, "9:35 AM"), camera(t, [[0, 1, 640, 300], [c.fade, 1.05, 640, 270]])) + `<div class="stage" style="background:var(--ink);opacity:${fade}"></div>`;
    },
  };

  /** Pixel blocks that fill the screen as a scene ends and clear as the next begins. */
  function wipe(local, length, first, last) {
    const leaving = last ? -1 : (local - (length - WIPE)) / WIPE;
    const arriving = first ? 2 : local / WIPE;
    if (leaving <= 0 && arriving >= 1) return "";
    const cells = [];
    for (let row = 0; row < 9; row++)
      for (let col = 0; col < 16; col++) {
        const v = ((col + row) / 23) * 0.82 + noise(col * 31 + row * 17) * 0.17 + 0.005;
        if (leaving > v || arriving < v) cells.push(`<i style="left:${col * 80}px;top:${row * 80}px;background:${noise(col * 5 + row * 11) > 0.86 ? "var(--highlight)" : "var(--ink)"}"></i>`);
      }
    return `<div class="wipe">${cells.join("")}</div>`;
  }

  window.render = (t) => {
    const scene = TOUR.find((x) => t >= x.start && t < x.end) ?? TOUR[TOUR.length - 1];
    const local = t - scene.start;
    const n = TOUR.indexOf(scene);
    const label = scene.id === "boot" || scene.id === "end" ? "Owlhead" : `Step ${n} of 5`;
    const k = back(span(local, 0.42, 0.4));
    const caption =
      local < 0.42 || (scene.id === "end" && local > C.end.fade)
        ? ""
        : `<div class="caption" style="transform:translate(-50%, ${(1 - k) * 46}px);opacity:${clamp(span(local, 0.42, 0.4) * 3, 0, 1)}"><b>${label}</b>${scene.caption}</div>`;
    document.getElementById("app").innerHTML = SCENES[scene.id](local) + caption + wipe(local, scene.end - scene.start, n === 0, n === TOUR.length - 1);
  };
})();
