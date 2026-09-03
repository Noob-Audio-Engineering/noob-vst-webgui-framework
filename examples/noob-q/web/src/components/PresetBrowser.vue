<script setup>
/**
 * Preset browser (manual §24), shown in a popover under the preset name:
 * folders (All / Factory / User / Favorites), search over names, tags and
 * folder, the list with favourite stars, a details column, Save As, delete
 * (user presets), and Copy / Paste of the whole state as JSON through the
 * clipboard.
 *
 * Emits `close`. No props.
 *
 * Presets are `{ name, author, tags, description, values }` where `values`
 * maps parameter id → plain value; `loadState` resets every parameter not
 * listed, so a preset only needs to name what it changes (see presets.js).
 * Loading skips the UI-only and demo-source parameters.
 *
 * User presets and favourites live in the plug-in's UI store under the
 * keys `presets.user` and `presets.favorites` (`client.store`), so they
 * persist with the plug-in state and are shared by every window of this
 * instance; `onPresetStoreChange` refreshes the lists when another window
 * saves, or when the host restores state.
 *
 * Keyboard while the browser has focus: Up / Down move the cursor, Enter
 * loads and closes, Right loads and keeps the browser open, `[` / `]` step,
 * Escape closes. Single click loads without closing, double-click loads
 * and closes (`load(p, close)`).
 */
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { loadState, stateToJson, ui } from '../composables/useVst3WebStratum.js';
import { FACTORY_PRESETS, loadFavorites, loadUserPresets, onPresetStoreChange, saveFavorites, saveUserPresets } from '../presets.js';

const emit = defineEmits(['close']);
const query = ref('');
const onlyFav = ref(false);
const user = ref(loadUserPresets());
const favs = ref(loadFavorites());
// Follow the store: another window saving, or the host restoring state.
const offStore = onPresetStoreChange(() => {
  user.value = loadUserPresets();
  favs.value = loadFavorites();
});
onBeforeUnmount(offStore);
const cursor = ref(0);
const folder = ref('All');

const all = computed(() => [
  ...FACTORY_PRESETS.map((p) => ({ ...p, folder: 'Factory' })),
  ...user.value.map((p) => ({ ...p, folder: 'User' })),
]);
const folders = ['All', 'Factory', 'User', 'Favorites'];
const list = computed(() => {
  const q = query.value.trim().toLowerCase();
  return all.value.filter((p) => {
    if (folder.value === 'Factory' && p.folder !== 'Factory') return false;
    if (folder.value === 'User' && p.folder !== 'User') return false;
    if ((folder.value === 'Favorites' || onlyFav.value) && !favs.value.has(p.name)) return false;
    if (!q) return true;
    return p.name.toLowerCase().includes(q) || (p.tags || []).some((t) => t.includes(q)) || p.folder.toLowerCase().includes(q);
  });
});
const current = computed(() => list.value[cursor.value] || null);
watch(list, () => (cursor.value = Math.min(cursor.value, Math.max(0, list.value.length - 1))));

/** Apply a preset (one frame, unlisted parameters reset) and make it the current one; `close` also dismisses the popover. */
function load(p, close = false) {
  loadState(p.values || {});
  ui.preset = { name: p.name, modified: false, index: all.value.findIndex((x) => x.name === p.name) };
  if (close) emit('close');
}
/** Star / unstar by name; favourites are a name list in the store (`presets.favorites`). */
function toggleFav(p) {
  const s = new Set(favs.value);
  if (s.has(p.name)) s.delete(p.name);
  else s.add(p.name);
  favs.value = s;
  saveFavorites(s);
}
function saveAs() {
  const name = window.prompt('Preset name', ui.preset.name === 'Default Setting' ? 'My Preset' : ui.preset.name);
  if (!name) return;
  const preset = { name, author: 'you', tags: ['user'], description: '', values: stateToJson() };
  const list = user.value.filter((p) => p.name !== name);
  list.push(preset);
  user.value = list;
  saveUserPresets(list);
  ui.preset = { name, modified: false, index: -1 };
}
function remove(p) {
  if (p.folder !== 'User') return;
  user.value = user.value.filter((x) => x.name !== p.name);
  saveUserPresets(user.value);
}
async function copyState() {
  try {
    await navigator.clipboard.writeText(JSON.stringify({ noobq: 1, name: ui.preset.name, values: stateToJson() }, null, 1));
  } catch {
    /* clipboard unavailable */
  }
}
async function pasteState() {
  try {
    const txt = await navigator.clipboard.readText();
    const j = JSON.parse(txt);
    if (j && j.values) {
      loadState(j.values);
      ui.preset = { name: j.name || 'Pasted', modified: true, index: -1 };
    }
  } catch {
    /* nothing usable */
  }
}
function onKey(e) {
  if (e.key === 'ArrowDown') {
    cursor.value = Math.min(list.value.length - 1, cursor.value + 1);
    e.preventDefault();
  } else if (e.key === 'ArrowUp') {
    cursor.value = Math.max(0, cursor.value - 1);
    e.preventDefault();
  } else if (e.key === 'Enter' && current.value) load(current.value, true);
  else if (e.key === 'ArrowRight' && current.value) load(current.value, false);
  else if (e.key === 'Escape') emit('close');
  else if (e.key === '[') cursor.value = Math.max(0, cursor.value - 1);
  else if (e.key === ']') cursor.value = Math.min(list.value.length - 1, cursor.value + 1);
}
</script>

<template>
  <div class="w-[560px] h-[340px] flex flex-col" tabindex="0" @keydown.stop="onKey">
    <div class="flex items-center gap-2 px-3 py-2 border-b border-white/10">
      <input v-model="query" placeholder="Type to search…" class="flex-1 bg-ink-950 border border-white/10 rounded px-2 py-1 text-[12px] outline-none focus:border-accent" @keydown.stop="onKey" />
      <button class="chip" :class="{ on: onlyFav }" title="Show only favourites" @click="onlyFav = !onlyFav">★</button>
    </div>
    <div class="flex-1 min-h-0 grid grid-cols-[110px_1fr_200px]">
      <div class="border-r border-white/10 py-1">
        <button v-for="f in folders" :key="f" class="w-full text-left px-3 py-1 text-[12px] hover:bg-white/[0.06]" :class="{ 'text-accent': folder === f }" @click="folder = f">{{ f }}</button>
      </div>
      <div class="overflow-y-auto py-1">
        <div
          v-for="(p, i) in list"
          :key="p.folder + p.name"
          class="flex items-center gap-2 px-3 py-1 text-[12px] cursor-pointer hover:bg-white/[0.06]"
          :class="{ 'bg-accent/20': i === cursor, 'text-accent': ui.preset.name === p.name }"
          @click="cursor = i; load(p, false)"
          @dblclick="load(p, true)"
        >
          <span class="flex-1 truncate">{{ p.name }}</span>
          <button class="text-[11px]" :class="favs.has(p.name) ? 'text-amber-300' : 'text-slate-600 hover:text-slate-300'" @click.stop="toggleFav(p)">★</button>
        </div>
        <div v-if="!list.length" class="px-3 py-4 text-slate-500 text-[12px]">No presets match.</div>
      </div>
      <div class="border-l border-white/10 p-3 text-[11px] flex flex-col gap-1.5">
        <template v-if="current">
          <div class="text-[13px] font-semibold text-slate-100">{{ current.name }}</div>
          <div class="text-slate-500">by {{ current.author }}</div>
          <div class="flex flex-wrap gap-1"><span v-for="t in current.tags" :key="t" class="px-1.5 rounded bg-white/[0.06] text-slate-300">{{ t }}</span></div>
          <div class="text-slate-400 leading-snug">{{ current.description }}</div>
          <button v-if="current.folder === 'User'" class="chip mt-auto self-start text-red-300" @click="remove(current)">Delete</button>
        </template>
      </div>
    </div>
    <div class="flex items-center gap-2 px-3 py-2 border-t border-white/10 text-[11px]">
      <button class="chip" @click="copyState">Copy</button>
      <button class="chip" @click="pasteState">Paste</button>
      <span class="text-slate-500 ml-2">↑↓ navigate · Enter load · → load and keep open · [ ] prev/next</span>
      <button class="chip on ml-auto" @click="saveAs">Save As…</button>
    </div>
  </div>
</template>

<style scoped>
@reference '../style.css';
.chip {
  @apply rounded px-2 py-0.5 text-[11px] border border-white/10 bg-white/[0.04] text-slate-300 cursor-pointer hover:bg-white/[0.08] transition-colors;
}
.chip.on {
  @apply bg-accent/90 text-ink-950 border-transparent font-semibold;
}
</style>
