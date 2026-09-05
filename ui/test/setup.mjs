import { registerHooks } from 'node:module';
import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
import { parse, compileScript } from '@vue/compiler-sfc';
import { createRenderer } from 'vue';

const sourceRoot = new URL('../src/', import.meta.url);
registerHooks({
  resolve(specifier, context, nextResolve) {
    let url;
    if (specifier.startsWith('@/')) url = new URL(specifier.slice(2), sourceRoot);
    else if (specifier.startsWith('.') && context.parentURL?.startsWith(sourceRoot.href)) url = new URL(specifier, context.parentURL);
    if (url && !/\.[a-z]+$/.test(url.pathname) && existsSync(fileURLToPath(url) + '.ts')) url = new URL(url.href + '.ts');
    return nextResolve(url?.href ?? specifier, context);
  },
  load(url, context, nextLoad) {
    if (!url.endsWith('.vue')) return nextLoad(url, context);
    const filename = fileURLToPath(url);
    const { descriptor } = parse(readFileSync(filename, 'utf8'));
    const script = compileScript(descriptor, { id: filename });
    return { format: 'module', shortCircuit: true, source: ts.transpileModule(script.content, { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } }).outputText };
  },
});

const values = new Map();
globalThis.localStorage = { getItem: key => values.get(key) ?? null, setItem: (key, value) => values.set(key, value), removeItem: key => values.delete(key), clear: () => values.clear() };
globalThis.window = { addEventListener() {}, removeEventListener() {} };

const renderer = createRenderer({ createComment: () => ({}), insert() {}, remove() {}, parentNode: () => null, nextSibling: () => null });
export function mountSetup(setup, providers = []) {
  let result;
  const app = renderer.createApp({ setup() { result = setup(); return () => null; } });
  for (const [key, value] of providers) app.provide(key, value);
  app.mount({});
  return { state: result, unmount: () => app.unmount() };
}
export function memory(overrides = {}) {
  return { id: 'one', content: 'Original', title: null, namespace: 'clio', kind: 'note', tags: [], importance: 3, updated_at: 'version-1', created_at: 'version-1', ...overrides };
}
