/**
 * Value formatting and text-entry parsing helpers shared by UI code.
 *
 * Pure functions, no Vue dependency: note names for frequency parameters,
 * MIDI conversions, and `parseValue()`, which turns what a user typed into a
 * knob's text field (`1k`, `A4`, `50%`, `2x`, `250ms`) into a plain value
 * for a parameter handle.
 *
 * Conventions: middle C is C4 (MIDI 60), A4 is 440 Hz (MIDI 69).
 */

/** Pitch-class names, sharps only. @private */
const NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];

/**
 * `{ name: 'A4', cents: +13, midi: 69 }` for a frequency. Middle C is C4.
 *
 * `midi` is the nearest note; `cents` is the signed offset from it, -50..50.
 * A non-positive or non-numeric frequency yields `{ name: '', cents: 0, midi: 0 }`.
 * @param {number} freq Hertz.
 * @returns {{ name: string, cents: number, midi: number }}
 */
export function freqToNote(freq) {
  if (!(freq > 0)) return { name: '', cents: 0, midi: 0 };
  const midiF = 69 + 12 * Math.log2(freq / 440);
  const midi = Math.round(midiF);
  const cents = Math.round((midiF - midi) * 100);
  const name = NAMES[((midi % 12) + 12) % 12] + (Math.floor(midi / 12) - 1);
  return { name, cents, midi };
}

/**
 * MIDI note number (fractional allowed) -> frequency in Hz, equal temperament, A4 = 440.
 * @param {number} midi
 * @returns {number}
 */
export function midiToFreq(midi) {
  return 440 * Math.pow(2, (midi - 69) / 12);
}

/**
 * MIDI note number → name, e.g. 60 → `C4`.
 * @param {number} midi Integer note number; negative octaves are handled.
 * @returns {string}
 */
export function noteName(midi) {
  return NAMES[((midi % 12) + 12) % 12] + (Math.floor(midi / 12) - 1);
}

/**
 * Parse `A4`, `C#3+13`, `D#5 -7` → frequency, or NaN.
 *
 * Accepts a letter A–G in either case, an optional `#` or `b`, an octave
 * (may be negative), and an optional cents offset with a sign. Whitespace
 * around the parts is ignored.
 * @param {string} text
 * @returns {number} Hertz, or `NaN` when the text is not a note.
 */
export function noteToFreq(text) {
  const m = /^\s*([A-Ga-g])([#b]?)(-?\d+)\s*([+-]\s*\d+)?\s*$/.exec(text);
  if (!m) return NaN;
  let semis = NAMES.indexOf(m[1].toUpperCase());
  if (m[2] === '#') semis += 1;
  if (m[2] === 'b') semis -= 1;
  const octave = parseInt(m[3], 10);
  const cents = m[4] ? parseInt(m[4].replace(/\s+/g, ''), 10) : 0;
  return midiToFreq((octave + 1) * 12 + semis + cents / 100);
}

/**
 * Note label with cents, e.g. `A4 +13`.
 *
 * The cents part is omitted when the frequency is within half a cent of the
 * note.
 * @param {number} freq Hertz.
 * @returns {string}
 */
export function noteLabel(freq) {
  const n = freqToNote(freq);
  return n.cents ? `${n.name} ${n.cents > 0 ? '+' : ''}${n.cents}` : n.name;
}

/**
 * Parse a typed value for a parameter handle. Understands `1k`, `A4`,
 * `C#3+13`, `2x` (dB: two times louder), `50%` (of the range), `250ms`,
 * plain numbers. Returns a plain value or NaN.
 *
 * Which shorthands apply depends on the handle's unit (case-insensitive):
 *
 * | unit        | extra forms                                            |
 * |-------------|--------------------------------------------------------|
 * | any         | `50%` = 50 % of the normalized range, via `h.toPlain`  |
 * | `Hz`        | `1k` / `1.5kHz` = kilohertz; note names via `noteToFreq` |
 * | `dB`        | `2x` = `20·log10(2)` dB (a gain ratio)                  |
 * | `s` / `ms`  | `250ms` / `0.5s`, converted to the handle's unit        |
 *
 * Anything else is parsed as a number; a decimal comma is accepted.
 * @param {string} text What the user typed.
 * @param {{ unit?: string, toPlain: (norm: number) => number }} h A parameter handle (or anything with `unit` and `toPlain`).
 * @returns {number} Plain value, or `NaN` when nothing usable was typed.
 */
export function parseValue(text, h) {
  const s = String(text).trim();
  if (!s) return NaN;
  const pct = /^(-?\d+(?:\.\d+)?)\s*%$/.exec(s);
  if (pct) return h.toPlain(Math.max(0, Math.min(100, parseFloat(pct[1]))) / 100);
  const unit = (h.unit || '').toLowerCase();
  if (unit === 'hz') {
    const k = /^(\d+(?:\.\d+)?)\s*k(?:hz)?$/i.exec(s);
    if (k) return parseFloat(k[1]) * 1000;
    const n = noteToFreq(s);
    if (!Number.isNaN(n)) return n;
  }
  if (unit === 'db') {
    const x = /^(\d+(?:\.\d+)?)\s*x$/i.exec(s);
    if (x) return 20 * Math.log10(parseFloat(x[1]));
  }
  if (unit === 's' || unit === 'ms') {
    const ms = /^(\d+(?:\.\d+)?)\s*ms$/i.exec(s);
    if (ms) return unit === 's' ? parseFloat(ms[1]) / 1000 : parseFloat(ms[1]);
    const sec = /^(\d+(?:\.\d+)?)\s*s$/i.exec(s);
    if (sec) return unit === 's' ? parseFloat(sec[1]) : parseFloat(sec[1]) * 1000;
  }
  const num = parseFloat(s.replace(',', '.'));
  return Number.isNaN(num) ? NaN : num;
}
