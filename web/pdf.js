// Minimal zero-dependency browser/Node PDF static text extractor.
// Mirrors src/pdf_parse.rs for uncompressed %PDF-1.x streams and supports
// async FlateDecode via standard Web DecompressionStream('deflate') when available.

const latin1Dec = new TextDecoder("latin1");

export function hasPdfHeader(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.length < 10) return false;
  return (
    bytes[0] === 0x25 && // %
    bytes[1] === 0x50 && // P
    bytes[2] === 0x44 && // D
    bytes[3] === 0x46 && // F
    bytes[4] === 0x2d    // -
  );
}

function unescapePdfLiteral(raw) {
  let out = "";
  for (let i = 0; i < raw.length; i++) {
    const ch = raw[i];
    if (ch === "\\" && i + 1 < raw.length) {
      i++;
      const nxt = raw[i];
      if (nxt === "n") out += "\n";
      else if (nxt === "r") out += "\r";
      else if (nxt === "t") out += "\t";
      else if (nxt === "(" || nxt === ")" || nxt === "\\") out += nxt;
      else if (nxt === "\r") {
        if (raw[i + 1] === "\n") i++;
      } else if (nxt === "\n") {
        // line continuation
      } else if (nxt >= "0" && nxt <= "7") {
        let oct = nxt;
        for (let k = 0; k < 2 && i + 1 < raw.length && raw[i + 1] >= "0" && raw[i + 1] <= "7"; k++) {
          oct += raw[++i];
        }
        out += String.fromCharCode(parseInt(oct, 8) & 0xff);
      } else {
        out += nxt;
      }
    } else {
      out += ch;
    }
  }
  return out;
}

function extractLinesFromStreamText(streamText) {
  const lines = [];
  let cur = "";
  let inText = false;
  let i = 0;
  const n = streamText.length;

  const flush = () => {
    const trimmed = cur.trim();
    if (trimmed) lines.push(trimmed);
    cur = "";
  };

  while (i < n) {
    const ch = streamText[i];
    if (/\s/.test(ch)) {
      i++;
      continue;
    }
    if (ch === "%") {
      while (i < n && streamText[i] !== "\n" && streamText[i] !== "\r") i++;
      continue;
    }
    if (ch === "(") {
      let depth = 1;
      let raw = "";
      i++;
      while (i < n && depth > 0) {
        const c = streamText[i];
        if (c === "\\" && i + 1 < n) {
          raw += c + streamText[i + 1];
          i += 2;
          continue;
        }
        if (c === "(") {
          depth++;
          raw += c;
        } else if (c === ")") {
          depth--;
          if (depth > 0) raw += c;
        } else {
          raw += c;
        }
        i++;
      }
      if (inText) {
        // Look ahead for Tj / ' / "
        let j = i;
        while (j < n && /\s/.test(streamText[j])) j++;
        if (streamText.startsWith("Tj", j)) {
          cur += unescapePdfLiteral(raw);
          i = j + 2;
        } else if (streamText[j] === "'" || streamText[j] === '"') {
          flush();
          cur += unescapePdfLiteral(raw);
          i = j + 1;
        }
      }
      continue;
    }
    if (ch === "[") {
      i++;
      let buf = "";
      while (i < n && streamText[i] !== "]") {
        if (streamText[i] === "(") {
          let depth = 1;
          let raw = "";
          i++;
          while (i < n && depth > 0) {
            const c = streamText[i];
            if (c === "\\" && i + 1 < n) {
              raw += c + streamText[i + 1];
              i += 2;
              continue;
            }
            if (c === "(") {
              depth++;
              raw += c;
            } else if (c === ")") {
              depth--;
              if (depth > 0) raw += c;
            } else {
              raw += c;
            }
            i++;
          }
          buf += unescapePdfLiteral(raw);
        } else if (/[-\d.]/.test(streamText[i])) {
          let start = i;
          while (i < n && /[-\d.]/.test(streamText[i])) i++;
          const num = parseFloat(streamText.slice(start, i));
          if (!Number.isNaN(num) && num < -180 && buf && !buf.endsWith(" ")) {
            buf += " ";
          }
        } else {
          i++;
        }
      }
      if (streamText[i] === "]") i++;
      let j = i;
      while (j < n && /\s/.test(streamText[j])) j++;
      if (inText && streamText.startsWith("TJ", j)) {
        cur += buf;
        i = j + 2;
      }
      continue;
    }
    // Operator word
    const start = i;
    while (i < n && !/[\s()<>[\]{}/%]/.test(streamText[i])) i++;
    const word = streamText.slice(start, i);
    if (word === "BT") {
      flush();
      inText = true;
    } else if (word === "ET") {
      flush();
      inText = false;
    } else if (word === "T*" && inText) {
      flush();
    } else if ((word === "Td" || word === "TD") && inText) {
      flush();
    }
    if (i === start) i++;
  }
  flush();
  return lines;
}

async function inflateZlib(rawBytes) {
  if (typeof DecompressionStream !== "undefined") {
    const ds = new DecompressionStream("deflate");
    const writer = ds.writable.getWriter();
    writer.write(rawBytes);
    writer.close();
    const out = await new Response(ds.readable).arrayBuffer();
    return new Uint8Array(out);
  }
  return null;
}

export async function extractPdfBinary(bytes) {
  if (!hasPdfHeader(bytes)) {
    return { status: "unsupported", reason: "invalid_pdf_header", text: "" };
  }
  const latin1 = latin1Dec.decode(bytes);
  if (/\/Encrypt\b/.test(latin1)) {
    return { status: "unsupported", reason: "encrypted_pdf", text: "" };
  }

  const lines = [];
  const objRegex = /(\d+)\s+\d+\s+obj\b([\s\S]*?)\bendobj\b/g;
  let match;
  let sawUnsupportedFilter = false;

  while ((match = objRegex.exec(latin1)) !== null) {
    const body = match[2];
    const sIdx = body.indexOf("stream");
    const eIdx = body.lastIndexOf("endstream");
    if (sIdx < 0 || eIdx < 0 || eIdx <= sIdx) continue;
    const dict = body.slice(0, sIdx);
    let dataStart = sIdx + 6;
    if (body[dataStart] === "\r" && body[dataStart + 1] === "\n") dataStart += 2;
    else if (body[dataStart] === "\n") dataStart += 1;
    let dataEnd = eIdx;
    if (dataEnd > dataStart && body[dataEnd - 1] === "\n") dataEnd--;
    if (dataEnd > dataStart && body[dataEnd - 1] === "\r") dataEnd--;

    const sliceStart = match.index + match[0].indexOf(body) + dataStart;
    const rawSlice = bytes.subarray(sliceStart, sliceStart + (dataEnd - dataStart));

    let streamText = null;
    if (/\/Filter\b/.test(dict)) {
      if (/\/FlateDecode\b/.test(dict)) {
        try {
          const inflated = await inflateZlib(rawSlice);
          if (inflated) {
            streamText = latin1Dec.decode(inflated);
          } else {
            sawUnsupportedFilter = true;
          }
        } catch {
          sawUnsupportedFilter = true;
        }
      } else {
        sawUnsupportedFilter = true;
      }
    } else {
      streamText = latin1Dec.decode(rawSlice);
    }

    if (streamText && streamText.includes("BT")) {
      lines.push(...extractLinesFromStreamText(streamText));
    }
  }

  if (lines.length === 0 && sawUnsupportedFilter) {
    return { status: "unsupported", reason: "unsupported_pdf_filter", text: "" };
  }
  return {
    status: lines.length > 0 ? "ok" : "no_match",
    reason: lines.length > 0 ? null : "no_extractable_text",
    text: lines.join("\n")
  };
}
