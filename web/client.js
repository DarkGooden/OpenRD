// OpenRD web client v0 scaffolding.
//
// Opens a WebTransport connection, opens the Control bidirectional
// stream, sends a stub ClientHello, reads frames back from the server,
// parses ServerHello and displays its fields.

const logEl = document.getElementById('log');
function log(s, kind) {
  const t = new Date().toISOString().split('T')[1].replace('Z', '');
  const line = document.createElement('div');
  line.textContent = `[${t}] ${s}`;
  if (kind === 'good') line.style.color = '#7fdb7f';
  if (kind === 'bad')  line.style.color = '#ff8b8b';
  if (kind === 'sys')  line.style.color = '#888';
  logEl.appendChild(line);
  logEl.scrollTop = logEl.scrollHeight;
}

document.getElementById('go').addEventListener('click', async () => {
  const url = document.getElementById('url').value;
  log(`connecting to ${url}...`, 'sys');

  if (!('WebTransport' in window)) {
    log('error: WebTransport not supported in this browser.', 'bad');
    log('try a recent Chromium-based browser.', 'sys');
    return;
  }

  let wt;
  try {
    wt = new WebTransport(url);
    await wt.ready;
    log('WebTransport ready', 'good');
  } catch (e) {
    log(`connect failed: ${e}`, 'bad');
    return;
  }

  let stream;
  try {
    stream = await wt.createBidirectionalStream();
    log('opened Control bidirectional stream', 'good');
  } catch (e) {
    log(`stream open failed: ${e}`, 'bad');
    return;
  }

  const writer = stream.writable.getWriter();
  const reader = stream.readable.getReader();

  // Build and send ClientHello (Control frame type 0x01).
  const payload = encodeClientHello({
    protocolVersion: 0,
    clientName: 'openrd-web/0.0.1',
  });
  const frame = encodeFrame(0x01, payload);
  await writer.write(frame);
  log(`sent ClientHello (frame ${frame.length} B, payload ${payload.length} B)`);

  // Read frames back and dispatch.
  try {
    await readFrames(reader, handleFrame);
  } catch (e) {
    log(`read error: ${e}`, 'bad');
  }
});

function handleFrame(frameBytes) {
  const version = frameBytes[0];
  const type    = frameBytes[1];
  const dv      = new DataView(frameBytes.buffer, frameBytes.byteOffset, frameBytes.byteLength);
  const length  = dv.getUint32(2, true);
  const payload = frameBytes.subarray(6, 6 + length);

  log(`recv frame: ver=${version} type=0x${type.toString(16).padStart(2,'0')} len=${length}`);

  if (type === 0x02) {
    // ServerHello
    try {
      const reader = new CborReader(payload);
      const value = reader.readValue();
      const fields = describeServerHello(value);
      log('ServerHello fields:', 'good');
      for (const [k, v] of fields) {
        log(`  ${k}: ${v}`, 'good');
      }
    } catch (e) {
      log(`ServerHello decode failed: ${e}`, 'bad');
    }
  } else if (type === 0x0C) {
    log('server sent an Error frame (type 0x0C)', 'bad');
  } else {
    log(`(no parser for control type 0x${type.toString(16).padStart(2,'0')} yet)`, 'sys');
  }
}

function describeServerHello(map) {
  const out = [];
  if (!(map instanceof Map)) {
    throw new Error('ServerHello is not a map');
  }
  const proto = map.get(1);
  const name  = map.get(2);
  const caps  = map.get(3);
  const sid   = map.get(4);
  const time  = map.get(5);
  if (proto !== undefined) out.push(['protocol_version', String(proto)]);
  if (name  !== undefined) out.push(['server_name',      JSON.stringify(name)]);
  if (caps  !== undefined) out.push(['capabilities',     caps instanceof Map ? `${caps.size} entries` : '?']);
  if (sid   !== undefined) out.push(['session_id',       sid instanceof Uint8Array ? hex(sid) : '?']);
  if (time  !== undefined) out.push(['server_time',      `${time} (${new Date(Number(time) * 1000).toISOString()})`]);
  return out;
}

async function readFrames(reader, onFrame) {
  let buf = new Uint8Array(0);
  while (true) {
    const { value, done } = await reader.read();
    if (done) {
      log('server stream ended', 'sys');
      return;
    }
    const next = new Uint8Array(buf.length + value.length);
    next.set(buf, 0);
    next.set(value, buf.length);
    buf = next;

    while (buf.length >= 6) {
      const dv = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
      const length = dv.getUint32(2, true);
      if (buf.length < 6 + length) break;
      const frame = buf.slice(0, 6 + length);
      buf = buf.slice(6 + length);
      onFrame(frame);
    }
  }
}

function encodeFrame(frameType, payload) {
  const out = new Uint8Array(6 + payload.length);
  out[0] = 0x00;                                       // version
  out[1] = frameType;                                  // type
  const dv = new DataView(out.buffer);
  dv.setUint32(2, payload.length, true);               // length, little-endian
  out.set(payload, 6);
  return out;
}

function encodeClientHello({ protocolVersion, clientName }) {
  // CBOR map with two keys: 1 -> protocol_version, 2 -> client_name.
  // (Capabilities and session_id_hint omitted in this stub.)
  const w = new CborWriter();
  w.writeMapHeader(2);
  w.writeUint(1); w.writeUint(protocolVersion);
  w.writeUint(2); w.writeString(clientName);
  return w.finish();
}

function hex(bytes) {
  return Array.from(bytes).map(b => b.toString(16).padStart(2, '0')).join('');
}

// -------- Minimal CBOR writer (subset: uint, bytes, text, array, map) ----

class CborWriter {
  constructor() { this.chunks = []; }
  finish() {
    const total = this.chunks.reduce((n, c) => n + c.length, 0);
    const out = new Uint8Array(total);
    let off = 0;
    for (const c of this.chunks) { out.set(c, off); off += c.length; }
    return out;
  }
  writeUint(n)         { this._typedNum(0x00, n); }
  writeMapHeader(n)    { this._typedNum(0xA0, n); }
  writeArrayHeader(n)  { this._typedNum(0x80, n); }
  writeBytes(b)        { this._typedNum(0x40, b.length); this.chunks.push(b); }
  writeString(s) {
    const enc = new TextEncoder().encode(s);
    this._typedNum(0x60, enc.length);
    this.chunks.push(enc);
  }
  _typedNum(majorBase, n) {
    if (n < 24) {
      this.chunks.push(new Uint8Array([majorBase | n]));
    } else if (n < 256) {
      this.chunks.push(new Uint8Array([majorBase | 24, n]));
    } else if (n < 65536) {
      const b = new Uint8Array(3);
      b[0] = majorBase | 25;
      new DataView(b.buffer).setUint16(1, n, false);
      this.chunks.push(b);
    } else if (n < 0x1_0000_0000) {
      const b = new Uint8Array(5);
      b[0] = majorBase | 26;
      new DataView(b.buffer).setUint32(1, n, false);
      this.chunks.push(b);
    } else {
      const b = new Uint8Array(9);
      b[0] = majorBase | 27;
      const dv = new DataView(b.buffer);
      // Split BigInt into hi/lo 32-bit halves.
      const big = BigInt(n);
      dv.setUint32(1, Number(big >> 32n), false);
      dv.setUint32(5, Number(big & 0xFFFFFFFFn), false);
      this.chunks.push(b);
    }
  }
}

// -------- Minimal CBOR reader (subset: uint, neg, bytes, text, array, map) ----

class CborReader {
  constructor(bytes) {
    this.bytes = bytes;
    this.pos = 0;
  }
  readValue() {
    if (this.pos >= this.bytes.length) throw new Error('CBOR: unexpected EOF');
    const initial = this.bytes[this.pos++];
    const major = initial >> 5;
    const info  = initial & 0x1f;
    const length = this._readLength(info);
    switch (major) {
      case 0: return Number(length);
      case 1: return -1 - Number(length);
      case 2: {
        const n = Number(length);
        const b = this.bytes.slice(this.pos, this.pos + n);
        this.pos += n;
        return b;
      }
      case 3: {
        const n = Number(length);
        const s = new TextDecoder().decode(this.bytes.subarray(this.pos, this.pos + n));
        this.pos += n;
        return s;
      }
      case 4: {
        const arr = [];
        for (let i = 0; i < Number(length); i++) arr.push(this.readValue());
        return arr;
      }
      case 5: {
        const m = new Map();
        for (let i = 0; i < Number(length); i++) {
          const k = this.readValue();
          const v = this.readValue();
          m.set(k, v);
        }
        return m;
      }
      case 7: {
        if (info === 20) return false;
        if (info === 21) return true;
        if (info === 22) return null;
        if (info === 23) return undefined;
        throw new Error(`CBOR: unsupported simple info ${info}`);
      }
      default:
        throw new Error(`CBOR: unsupported major type ${major}`);
    }
  }
  _readLength(info) {
    if (info < 24) return info;
    if (info === 24) return this.bytes[this.pos++];
    if (info === 25) {
      const v = (this.bytes[this.pos] << 8) | this.bytes[this.pos + 1];
      this.pos += 2;
      return v;
    }
    if (info === 26) {
      const v = this.bytes[this.pos] * 0x1000000
              + ((this.bytes[this.pos + 1] << 16) | (this.bytes[this.pos + 2] << 8) | this.bytes[this.pos + 3]);
      this.pos += 4;
      return v;
    }
    if (info === 27) {
      const dv = new DataView(this.bytes.buffer, this.bytes.byteOffset + this.pos, 8);
      const hi = BigInt(dv.getUint32(0, false));
      const lo = BigInt(dv.getUint32(4, false));
      this.pos += 8;
      return (hi << 32n) | lo;
    }
    throw new Error(`CBOR: unsupported length encoding ${info}`);
  }
}
