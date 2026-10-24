// Optional Node oracle: executes verbatim source fixtures using only import/export removal.
// Normal cargo tests consume oracle.json and never need Node.
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import vm from 'node:vm';
const here = new URL('./', import.meta.url);
const cases = [];
const add = (command, store, payload = {}) => cases.push({command, store, payload});
for (const command of ['increment-counter', 'decrement-counter', 'reset-counter']) {
  add(command, {});
  for (const counter of [null, 0, -1, 12.8, 1e21, -1e21, 1e20, 1e-7, 1e-6,
    9007199254740992, -9007199254740992, 1.7976931348623157e308,
    '  +42tail', '-0xFxyz', '+0Xff!', '0b11', '08', '12e4', '-0',
    '900719925474099267', '0x20000000000001', '0x20000000000003',
    '9'.repeat(400), '', 'bad', true, [], {},
    '\uFEFF\u00a0\u2028\u3000-27.9', '\u008542', '\u180e42']) {
    add(command, {counter, future: {keep: true}});
  }
  for (const noUpdate of [true, 'false', [], {}, false, null, '', 0, 1]) {
    add(command, {}, {launchContext: {noUpdate}});
    add(command, {counter:' 0x10suffix', future: 9}, {launchContext:{noUpdate}});
  }
}
const results = [];
for (const item of cases) {
  let store = structuredClone(item.store);
  let writes = 0;
  const metadata = [];
  const source = readFileSync(new URL(`fixtures/v1/${item.command}.jsx`, here), 'utf8')
    .replace(/^import .*;\r?\n/gm, '')
    .replace('export default async function Command', 'async function Command');
  const context = vm.createContext({LocalStorage: {
    getItem: async key => store[key],
    setItem: async (key, value) => { store[key] = value; writes++; }
  }, updateCommandMetadata: async value => metadata.push(value)});
  vm.runInContext(source, context);
  await context.Command(structuredClone(item.payload));
  const validType = !('counter' in item.store) || item.store.counter === null ||
    ['string','number'].includes(typeof item.store.counter);
  const parsed = parseInt(item.store.counter ?? 0);
  const noUpdate = item.command !== 'reset-counter' && !!item.payload.launchContext?.noUpdate;
  const value = item.command === 'reset-counter' ? 0 : parsed + (noUpdate ? 0 : item.command === 'increment-counter' ? 1 : -1);
  const nativeError = item.command !== 'reset-counter' && (!validType || !Number.isFinite(value));
  results.push({...item, expected:{store, writes, metadata, value, nativeError}});
}
const serialized = JSON.stringify(results, null, 2) + '\n';
const fixture = new URL('oracle.json', here);
if (process.argv.includes('--write')) writeFileSync(fixture, serialized);
else assert.equal(readFileSync(fixture, 'utf8'), serialized, 'oracle changed; inspect source/expectations');
console.log(`Actual-source oracle: ${results.length} cases verified`);
