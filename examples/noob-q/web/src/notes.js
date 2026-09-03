/**
 * Frequency ↔ note-name helpers and text-entry parsing, re-exported from
 * the framework so the components import them from one local place:
 *
 * - `freqToNote(hz)` → `{ midi, name, cents }` (nearest note and the offset)
 * - `midiToFreq(midi)` / `noteToFreq('A#3')` → Hz
 * - `noteLabel(hz)` → e.g. "A4 +12" for the piano display
 * - `parseValue(text, handle)` → plain value from typed text, understanding
 *   units ("2k", "-3 dB") and note names, or NaN when unusable
 */
export { freqToNote, midiToFreq, noteToFreq, noteLabel, parseValue } from '@elyerinfox/vst3-web-stratum/vue';
