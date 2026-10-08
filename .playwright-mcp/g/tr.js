(() => {
  var __defProp = Object.defineProperty;
  var __getOwnPropNames = Object.getOwnPropertyNames;
  var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
  var __hasOwnProp = Object.prototype.hasOwnProperty;
  function __accessProp(key) {
    return this[key];
  }
  var __toCommonJS = (from) => {
    var entry = (__moduleCache ??= new WeakMap).get(from), desc;
    if (entry)
      return entry;
    entry = __defProp({}, "__esModule", { value: true });
    if (from && typeof from === "object" || typeof from === "function") {
      for (var key of __getOwnPropNames(from))
        if (!__hasOwnProp.call(entry, key))
          __defProp(entry, key, {
            get: __accessProp.bind(from, key),
            enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable
          });
    }
    __moduleCache.set(from, entry);
    return entry;
  };
  var __moduleCache;
  var __returnValue = (v) => v;
  function __exportSetter(name, newValue) {
    this[name] = __returnValue.bind(null, newValue);
  }
  var __export = (target, all) => {
    for (var name in all)
      __defProp(target, name, {
        get: all[name],
        enumerable: true,
        configurable: true,
        set: __exportSetter.bind(all, name)
      });
  };

  // src/engine/text/text-renderer.ts
  var exports_text_renderer = {};
  __export(exports_text_renderer, {
    textSpecBounds: () => textSpecBounds,
    textObjectQuad: () => textObjectQuad,
    specFontSize: () => specFontSize,
    specAabb: () => specAabb,
    scaleRunSizes: () => scaleRunSizes,
    runsPlainText: () => runsPlainText,
    rasterizeTexts: () => rasterizeTexts,
    fontShorthand: () => fontShorthand,
    balloonPad: () => balloonPad,
    balloonExtraLocalPoints: () => balloonExtraLocalPoints,
    DEFAULT_TEXT: () => DEFAULT_TEXT
  });
  var DEFAULT_TEXT = {
    fontFamily: "system-ui, sans-serif",
    fontSizePx: 32,
    align: "left",
    lineHeight: 1.2,
    bold: false,
    italic: false,
    letterSpacing: 0,
    rotation: 0
  };
  function fontShorthand(spec) {
    const style = spec.italic ? "italic " : "";
    const weight = spec.bold ? "700 " : "400 ";
    return `${style}${weight}${spec.fontSizePx}px ${spec.fontFamily}`;
  }
  function runsPlainText(runs) {
    return runs.map((r) => r.text).join("");
  }
  function scaleRunSizes(runs, k) {
    return runs?.map((r) => r.fontSizePx != null ? { ...r, fontSizePx: r.fontSizePx * k } : r);
  }
  var runSize = (spec, run) => run.fontSizePx ?? spec.fontSizePx;
  function runFontShorthand(spec, run) {
    const style = run.italic ?? spec.italic ? "italic " : "";
    const weight = run.bold ?? spec.bold ? "700 " : "400 ";
    return `${style}${weight}${runSize(spec, run)}px ${run.fontFamily ?? spec.fontFamily}`;
  }
  function applyRunFont(ctx, spec, run) {
    ctx.font = runFontShorthand(spec, run);
    ctx.textBaseline = "alphabetic";
    ctx.textAlign = "left";
    try {
      ctx.letterSpacing = `${spec.letterSpacing}px`;
    } catch {}
  }
  function layoutRuns(ctx, spec) {
    const measure = (text, run) => {
      applyRunFont(ctx, spec, run);
      return ctx.measureText(text).width;
    };
    const tokens = [];
    for (const run of spec.runs ?? []) {
      for (const part of run.text.split(/(\n)/)) {
        if (!part)
          continue;
        if (part === `
`) {
          tokens.push({ frags: [], width: 0, nl: true });
          continue;
        }
        for (const piece of part.split(/(\s+)/)) {
          if (!piece)
            continue;
          const ws = /^\s+$/.test(piece);
          const frag = { text: piece, run, width: measure(piece, run), ws };
          const last = tokens[tokens.length - 1];
          if (!ws && last && !last.ws && !last.nl) {
            last.frags.push(frag);
            last.width += frag.width;
          } else {
            tokens.push({ frags: [frag], width: frag.width, ws });
          }
        }
      }
    }
    const maxW = spec.boxWidth && spec.boxWidth > 0 ? spec.boxWidth : Infinity;
    const raw = [];
    let cur = { frags: [], width: 0 };
    const pushLine = () => {
      while (cur.frags.length > 0 && cur.frags[cur.frags.length - 1].ws) {
        cur.width -= cur.frags.pop().width;
      }
      raw.push(cur);
      cur = { frags: [], width: 0 };
    };
    for (const t of tokens) {
      if (t.nl) {
        pushLine();
        continue;
      }
      if (cur.width + t.width > maxW && cur.frags.some((f) => !f.ws)) {
        pushLine();
        if (t.ws)
          continue;
      }
      cur.frags.push(...t.frags);
      cur.width += t.width;
    }
    pushLine();
    let maxSizeAll = 0;
    const lines = raw.map((l) => {
      const maxSize = l.frags.length ? Math.max(...l.frags.map((f) => runSize(spec, f.run))) : spec.fontSizePx;
      maxSizeAll = Math.max(maxSizeAll, maxSize);
      const height = maxSize * spec.lineHeight;
      const halfLeading = (height - maxSize) / 2;
      let ascent = 0.8 * maxSize;
      const big = l.frags.find((f) => runSize(spec, f.run) === maxSize);
      if (big) {
        applyRunFont(ctx, spec, big.run);
        const ma = ctx.measureText("Mg");
        ctx.textBaseline = "top";
        const mt = ctx.measureText("Mg");
        ctx.textBaseline = "alphabetic";
        if (ma.fontBoundingBoxAscent && typeof mt.fontBoundingBoxAscent === "number") {
          ascent = ma.fontBoundingBoxAscent - mt.fontBoundingBoxAscent;
        }
      }
      return { frags: l.frags, width: l.width, height, baseline: halfLeading + ascent };
    });
    const blockW = spec.boxWidth ?? Math.max(0, ...lines.map((l) => l.width));
    const blockH = lines.reduce((s, l) => s + l.height, 0);
    return { lines, blockW, blockH, maxSize: maxSizeAll || spec.fontSizePx };
  }
  function drawSpecRuns(ctx, spec) {
    const L = layoutRuns(ctx, spec);
    ctx.save();
    ctx.translate(spec.x, spec.y);
    if (spec.rotation)
      ctx.rotate(spec.rotation);
    const pad = spec.bgColor ? L.maxSize * 0.18 : 0;
    if (spec.bgColor) {
      ctx.fillStyle = spec.bgColor;
      ctx.fillRect(-pad, -pad, L.blockW + pad * 2, L.blockH + pad * 2);
    }
    drawBalloon(ctx, spec, L.blockW, L.blockH);
    const ow = spec.outlineWidth ?? 0;
    let top = 0;
    for (const line of L.lines) {
      const left = spec.align === "center" ? (L.blockW - line.width) / 2 : spec.align === "right" ? L.blockW - line.width : 0;
      const by = top + line.baseline;
      if (ow > 0) {
        ctx.strokeStyle = spec.outlineColor ?? "#000000";
        ctx.lineWidth = ow * 2;
        ctx.lineJoin = "round";
        ctx.miterLimit = 2;
        let x2 = left;
        for (const f of line.frags) {
          applyRunFont(ctx, spec, f.run);
          ctx.strokeText(f.text, x2, by);
          x2 += f.width;
        }
      }
      let x = left;
      for (const f of line.frags) {
        applyRunFont(ctx, spec, f.run);
        ctx.fillStyle = f.run.color ?? spec.color;
        ctx.fillText(f.text, x, by);
        const size = runSize(spec, f.run);
        const thick = Math.max(1, size * 0.06);
        if (f.run.underline)
          ctx.fillRect(x, by + size * 0.1, f.width, thick);
        if (f.run.strikethrough)
          ctx.fillRect(x, by - size * 0.28, f.width, thick);
        x += f.width;
      }
      top += line.height;
    }
    ctx.restore();
    return specAabb(spec, L.blockW, L.blockH, Math.max(pad, ow));
  }
  function blockMetrics(ctx, spec) {
    if (spec.runs && spec.runs.length > 0) {
      const L2 = layoutRuns(ctx, spec);
      return { blockW: L2.blockW, blockH: L2.blockH };
    }
    const L = layoutSpec(ctx, spec);
    return { blockW: L.blockW, blockH: L.blockH };
  }
  function layoutLines(ctx, text, maxWidth) {
    const paragraphs = text.split(`
`);
    if (!maxWidth || maxWidth <= 0)
      return paragraphs;
    const out = [];
    for (const para of paragraphs) {
      const words = para.split(/(\s+)/);
      let line = "";
      for (const w of words) {
        const trial = line + w;
        if (ctx.measureText(trial).width > maxWidth && line.trim() !== "") {
          out.push(line.replace(/\s+$/, ""));
          line = w.replace(/^\s+/, "");
        } else {
          line = trial;
        }
      }
      out.push(line.replace(/\s+$/, ""));
    }
    return out;
  }
  function applyFont(ctx, spec) {
    ctx.font = fontShorthand(spec);
    ctx.textBaseline = "top";
    ctx.textAlign = spec.align;
    try {
      ctx.letterSpacing = `${spec.letterSpacing}px`;
    } catch {}
  }
  function layoutSpec(ctx, spec) {
    applyFont(ctx, spec);
    const lines = layoutLines(ctx, spec.text, spec.boxWidth);
    const lineH = spec.fontSizePx * spec.lineHeight;
    const halfLeading = (lineH - spec.fontSizePx) / 2;
    const blockW = spec.boxWidth ?? Math.max(0, ...lines.map((l) => ctx.measureText(l).width));
    const anchorDx = spec.align === "center" ? blockW / 2 : spec.align === "right" ? blockW : 0;
    return { lines, lineH, halfLeading, blockW, blockH: lines.length * lineH, anchorDx };
  }
  function specAabb(spec, blockW, blockH, pad = 0, extraLocal = []) {
    const cos = Math.cos(spec.rotation);
    const sin = Math.sin(spec.rotation);
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const [lx, ly] of [
      [-pad, -pad],
      [blockW + pad, -pad],
      [-pad, blockH + pad],
      [blockW + pad, blockH + pad],
      ...extraLocal
    ]) {
      const x = spec.x + lx * cos - ly * sin;
      const y = spec.y + lx * sin + ly * cos;
      minX = Math.min(minX, x);
      minY = Math.min(minY, y);
      maxX = Math.max(maxX, x);
      maxY = Math.max(maxY, y);
    }
    return { minX, minY, maxX, maxY };
  }
  function specFontSize(spec) {
    return spec.runs?.length ? Math.max(...spec.runs.map((r) => r.fontSizePx ?? spec.fontSizePx)) : spec.fontSizePx;
  }
  function balloonPad(spec, fontSize) {
    const b = spec.balloon;
    if (!b)
      return 0;
    return fontSize * (b.pad ?? 0.55) + (b.stroke ? (b.strokeWidth ?? 2) / 2 : 0);
  }
  function balloonExtraLocalPoints(spec) {
    const t = spec.balloon?.tail;
    if (!t)
      return [];
    const half = Math.max(0, (t.width ?? 24) / 2);
    return [
      [t.x, t.y],
      [t.x - half, t.y],
      [t.x + half, t.y],
      [t.x, t.y - half],
      [t.x, t.y + half]
    ];
  }
  function specPad(spec) {
    const padSize = specFontSize(spec);
    const bg = spec.bgColor ? padSize * 0.18 : 0;
    return Math.max(bg, spec.outlineWidth ?? 0, balloonPad(spec, padSize));
  }
  function lineLeft(align, anchorDx, lineW) {
    return align === "center" ? anchorDx - lineW / 2 : align === "right" ? anchorDx - lineW : anchorDx;
  }
  function drawBalloon(ctx, spec, blockW, blockH) {
    const b = spec.balloon;
    if (!b)
      return;
    const pad = specFontSize(spec) * (b.pad ?? 0.55);
    const x0 = -pad;
    const y0 = -pad;
    const w = blockW + pad * 2;
    const h = blockH + pad * 2;
    const cx = x0 + w / 2;
    const cy = y0 + h / 2;
    ctx.beginPath();
    if (b.shape === "rounded") {
      const r = Math.min(w, h) * 0.28;
      ctx.moveTo(x0 + r, y0);
      ctx.arcTo(x0 + w, y0, x0 + w, y0 + h, r);
      ctx.arcTo(x0 + w, y0 + h, x0, y0 + h, r);
      ctx.arcTo(x0, y0 + h, x0, y0, r);
      ctx.arcTo(x0, y0, x0 + w, y0, r);
      ctx.closePath();
    } else if (b.shape === "burst") {
      const spikes = 16;
      for (let i = 0;i < spikes * 2; i++) {
        const t = i / (spikes * 2) * Math.PI * 2;
        const k = i % 2 === 0 ? 1.22 : 0.86;
        const px = cx + Math.cos(t) * (w / 2) * k;
        const py = cy + Math.sin(t) * (h / 2) * k;
        if (i === 0)
          ctx.moveTo(px, py);
        else
          ctx.lineTo(px, py);
      }
      ctx.closePath();
    } else if (b.shape === "cloud") {
      const lobes = 12;
      const rr = Math.min(w, h) * 0.19;
      for (let i = 0;i < lobes; i++) {
        const t = i / lobes * Math.PI * 2;
        ctx.moveTo(cx + Math.cos(t) * (w / 2) + rr, cy + Math.sin(t) * (h / 2));
        ctx.arc(cx + Math.cos(t) * (w / 2), cy + Math.sin(t) * (h / 2), rr, 0, Math.PI * 2);
      }
      ctx.moveTo(cx + w / 2, cy);
      ctx.ellipse(cx, cy, w / 2, h / 2, 0, 0, Math.PI * 2);
    } else {
      ctx.moveTo(cx + w / 2, cy);
      ctx.ellipse(cx, cy, w / 2, h / 2, 0, 0, Math.PI * 2);
    }
    if (b.tail) {
      const tw = Math.max(1, b.tail.width ?? 24) / 2;
      const dx = b.tail.x - cx;
      const dy = b.tail.y - cy;
      const len = Math.hypot(dx, dy) || 1;
      const px = -dy / len * tw;
      const py = dx / len * tw;
      ctx.moveTo(cx + px, cy + py);
      ctx.lineTo(b.tail.x, b.tail.y);
      ctx.lineTo(cx - px, cy - py);
      ctx.closePath();
    }
    ctx.fillStyle = b.fill;
    ctx.fill();
    if (b.stroke) {
      ctx.strokeStyle = b.stroke;
      ctx.lineWidth = b.strokeWidth ?? 2;
      ctx.lineJoin = "round";
      ctx.stroke();
    }
  }
  function drawSpec(ctx, spec) {
    if (spec.text.length === 0)
      return null;
    if (spec.runs && spec.runs.length > 0)
      return drawSpecRuns(ctx, spec);
    const L = layoutSpec(ctx, spec);
    ctx.save();
    ctx.translate(spec.x, spec.y);
    if (spec.rotation)
      ctx.rotate(spec.rotation);
    if (spec.bgColor) {
      const pad = spec.fontSizePx * 0.18;
      ctx.fillStyle = spec.bgColor;
      ctx.fillRect(-pad, -pad, L.blockW + pad * 2, L.blockH + pad * 2);
    }
    drawBalloon(ctx, spec, L.blockW, L.blockH);
    const ow = spec.outlineWidth ?? 0;
    if (ow > 0) {
      ctx.strokeStyle = spec.outlineColor ?? "#000000";
      ctx.lineWidth = ow * 2;
      ctx.lineJoin = "round";
      ctx.miterLimit = 2;
      L.lines.forEach((line, i) => ctx.strokeText(line, L.anchorDx, i * L.lineH + L.halfLeading));
    }
    ctx.fillStyle = spec.color;
    L.lines.forEach((line, i) => ctx.fillText(line, L.anchorDx, i * L.lineH + L.halfLeading));
    if (spec.underline) {
      const thick = Math.max(1, spec.fontSizePx * 0.06);
      ctx.fillStyle = spec.color;
      L.lines.forEach((line, i) => {
        if (!line)
          return;
        const lw = ctx.measureText(line).width;
        const x = lineLeft(spec.align, L.anchorDx, lw);
        const y = i * L.lineH + L.halfLeading + spec.fontSizePx * 0.92;
        ctx.fillRect(x, y, lw, thick);
      });
    }
    ctx.restore();
    return specAabb(spec, L.blockW, L.blockH, specPad(spec));
  }
  function bboxOf(a, docWidth, docHeight, pad = 2) {
    const x = Math.max(0, Math.floor(a.minX - pad));
    const y = Math.max(0, Math.floor(a.minY - pad));
    const width = Math.min(docWidth, Math.ceil(a.maxX + pad)) - x;
    const height = Math.min(docHeight, Math.ceil(a.maxY + pad)) - y;
    if (width <= 0 || height <= 0)
      return null;
    return { x, y, width, height };
  }
  var measureCtx;
  function measuringCtx() {
    if (measureCtx === undefined)
      measureCtx = document.createElement("canvas").getContext("2d");
    return measureCtx;
  }
  function textSpecBounds(spec) {
    if (spec.text.length === 0)
      return null;
    const ctx = measuringCtx();
    if (!ctx)
      return null;
    const L = blockMetrics(ctx, spec);
    const a = specAabb(spec, L.blockW, L.blockH, specPad(spec), balloonExtraLocalPoints(spec));
    return {
      x: Math.floor(a.minX),
      y: Math.floor(a.minY),
      width: Math.ceil(a.maxX) - Math.floor(a.minX),
      height: Math.ceil(a.maxY) - Math.floor(a.minY)
    };
  }
  function textObjectQuad(spec) {
    if (spec.text.length === 0)
      return null;
    const ctx = measuringCtx();
    if (!ctx)
      return null;
    const L = blockMetrics(ctx, spec);
    const cos = Math.cos(spec.rotation);
    const sin = Math.sin(spec.rotation);
    const at = (lx, ly) => ({
      x: spec.x + lx * cos - ly * sin,
      y: spec.y + lx * sin + ly * cos
    });
    const extra = balloonExtraLocalPoints(spec);
    if (!spec.balloon && extra.length === 0) {
      return {
        corners: [at(0, 0), at(L.blockW, 0), at(L.blockW, L.blockH), at(0, L.blockH)],
        center: at(L.blockW / 2, L.blockH / 2)
      };
    }
    const pad = balloonPad(spec, specFontSize(spec));
    let lo = { x: -pad, y: -pad };
    let hi = { x: L.blockW + pad, y: L.blockH + pad };
    for (const [lx, ly] of extra) {
      lo = { x: Math.min(lo.x, lx), y: Math.min(lo.y, ly) };
      hi = { x: Math.max(hi.x, lx), y: Math.max(hi.y, ly) };
    }
    return {
      corners: [at(lo.x, lo.y), at(hi.x, lo.y), at(hi.x, hi.y), at(lo.x, hi.y)],
      center: at((lo.x + hi.x) / 2, (lo.y + hi.y) / 2)
    };
  }
  function rasterizeTexts(specs, docWidth, docHeight, ss = 1) {
    const canvas = document.createElement("canvas");
    canvas.width = docWidth * ss;
    canvas.height = docHeight * ss;
    const empty = { x: 0, y: 0, width: 0, height: 0 };
    const ctx = canvas.getContext("2d");
    if (!ctx)
      return { canvas, bbox: empty };
    if (ss !== 1)
      ctx.scale(ss, ss);
    let union = null;
    for (const spec of specs) {
      const a = drawSpec(ctx, spec);
      if (!a)
        continue;
      union = union ? {
        minX: Math.min(union.minX, a.minX),
        minY: Math.min(union.minY, a.minY),
        maxX: Math.max(union.maxX, a.maxX),
        maxY: Math.max(union.maxY, a.maxY)
      } : a;
    }
    return { canvas, bbox: (union && bboxOf(union, docWidth, docHeight)) ?? empty };
  }
})();
