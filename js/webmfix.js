// Browser recordings (MediaRecorder) are written live, so the WebM carries no duration:
// players then show no length and seeking barely works. This writes the duration into
// Segment → Info afterwards – in the browser, instead of remuxing with ffmpeg.
// If the file looks unexpected, it is returned unchanged.

const ID_EBML = 0x1a45dfa3;
const ID_SEGMENT = 0x18538067;
const ID_SEEKHEAD = 0x114d9b74;
const ID_INFO = 0x1549a966;
const ID_TIMESCALE = 0x2ad7b1;
const ID_DURATION = 0x4489;
const ID_CLUSTER = 0x1f43b675;

function vlen(b) {
  for (let n = 1; n <= 8; n++) if (b & (0x100 >> n)) return n;
  throw new Error("Bad EBML number");
}

// One element head: id, size (unknown = -1) and where its data starts
function readEl(buf, pos) {
  const idLen = vlen(buf[pos]);
  let id = 0;
  for (let i = 0; i < idLen; i++) id = id * 256 + buf[pos + i];
  const sizeStart = pos + idLen;
  const sizeLen = vlen(buf[sizeStart]);
  let size = buf[sizeStart] & (0xff >> sizeLen);
  let allOnes = size === 0xff >> sizeLen;
  for (let i = 1; i < sizeLen; i++) {
    const b = buf[sizeStart + i];
    size = size * 256 + b;
    if (b !== 0xff) allOnes = false;
  }
  return { id, sizeStart, sizeLen, size: allOnes ? -1 : size, dataStart: sizeStart + sizeLen };
}

function encodeSize(value, len) {
  if (value >= 2 ** (7 * len) - 1) return null;
  const out = new Uint8Array(len);
  for (let i = len - 1; i >= 0; i--) {
    out[i] = value % 256;
    value = Math.floor(value / 256);
  }
  out[0] |= 0x80 >> (len - 1);
  return out;
}

export async function fixWebmDuration(blob, ms) {
  if (!blob || !(ms > 0)) return blob;
  try {
    const head = new Uint8Array(await blob.slice(0, Math.min(blob.size, 1 << 20)).arrayBuffer());
    const ebml = readEl(head, 0);
    if (ebml.id !== ID_EBML) return blob;
    const seg = readEl(head, ebml.dataStart + ebml.size);
    if (seg.id !== ID_SEGMENT) return blob;
    const end = seg.size < 0 ? head.length : Math.min(head.length, seg.dataStart + seg.size);
    let seek = false;
    for (let p = seg.dataStart; p < end; ) {
      const el = readEl(head, p);
      if (el.id === ID_SEEKHEAD) seek = true;
      if (el.id === ID_INFO) return patchInfo(blob, head, el, ms, seek);
      if (el.id === ID_CLUSTER || el.size < 0) break;
      p = el.dataStart + el.size;
    }
  } catch (e) {
    console.warn("[PMV Generator] WebM duration", e);
  }
  return blob;
}

function patchInfo(blob, head, info, ms, seek) {
  const infoEnd = info.dataStart + info.size;
  if (infoEnd > head.length) return blob;
  let scale = 1e6;
  let dur = null;
  for (let p = info.dataStart; p < infoEnd; ) {
    const el = readEl(head, p);
    if (el.id === ID_TIMESCALE) {
      scale = 0;
      for (let i = 0; i < el.size; i++) scale = scale * 256 + head[el.dataStart + i];
    }
    if (el.id === ID_DURATION) dur = el;
    p = el.dataStart + el.size;
  }
  const value = (ms * 1e6) / (scale || 1e6);
  const rest = blob.slice(head.length);

  // Already has a duration field: overwrite it in place
  if (dur && (dur.size === 4 || dur.size === 8)) {
    const out = head.slice();
    const dv = new DataView(out.buffer, dur.dataStart, dur.size);
    if (dur.size === 8) dv.setFloat64(0, value);
    else dv.setFloat32(0, value);
    return new Blob([out, rest], { type: blob.type });
  }
  // Insert one – shifts everything behind it, so not when a SeekHead points to positions
  if (dur || seek) return blob;
  const el = new Uint8Array(11);
  el.set([0x44, 0x89, 0x88]);
  new DataView(el.buffer).setFloat64(3, value);
  const size = encodeSize(info.size + el.length, info.sizeLen);
  if (!size) return blob;
  return new Blob([head.subarray(0, info.sizeStart), size, head.subarray(info.dataStart, infoEnd), el, head.subarray(infoEnd), rest], { type: blob.type });
}
