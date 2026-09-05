// Disposable browser fixture: every Tauri command is intercepted before app startup.
import { createServer } from 'vite';
const server = await createServer({
  root: new URL('../', import.meta.url).pathname,
  server: { host: '127.0.0.1', port: 1428, strictPort: true, cors: false, fs: { strict: true } },
  plugins: [{ name: 'clio-disposable-fixture', transformIndexHtml(html) {
    return html.replace('<script type="module" src="/src/main.ts"></script>', '<script type="module" src="/test/fixture-backend.js"></script>');
  } }],
});
await server.listen();
console.log('Disposable Clio fixture: http://127.0.0.1:1428 (no live backend)');
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, async () => { await server.close(); process.exit(0); });
