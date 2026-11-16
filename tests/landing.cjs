const { JSDOM, VirtualConsole } = require('jsdom');
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const root = path.resolve(__dirname, '../landing');
(async () => {
  const errors = [];
  const logs = new VirtualConsole();
  logs.on('jsdomError', e => errors.push(e.message));
  const dom = new JSDOM(fs.readFileSync(path.join(root, 'index.html'), 'utf8'), {
    url: 'https://tako.example/', runScripts: 'outside-only', virtualConsole: logs
  });
  const w = dom.window, d = w.document;
  let copied = '';
  w.matchMedia = () => ({ matches: true });
  Object.defineProperty(w.navigator, 'clipboard', { configurable: true, value: { writeText: async s => { copied = s; } } });
  w.eval(fs.readFileSync(path.join(root, 'app.js'), 'utf8'));
  for (const a of d.querySelectorAll('a[href^="#"]')) {
    if (a.hash) assert.ok(d.querySelector(a.hash), `Anchor ${a.hash} exists`);
  }
  for (const e of d.querySelectorAll('[src], link[href]')) {
    const ref = e.getAttribute('src') || e.getAttribute('href');
    assert.ok(fs.existsSync(path.join(root, ref)), `Local asset ${ref} exists`);
  }
  assert.equal(d.querySelector('.endpoint'), null);
  assert.ok(!d.body.textContent.includes('base_url'));
  assert.ok(!d.body.textContent.includes('127.0.0.1:20129'));
  assert.match(d.querySelector('#compare').textContent, /OmniRoute/);
  d.querySelector('#copy').click();
  await new Promise(resolve => setTimeout(resolve, 0));
  assert.equal(copied, d.querySelector('#install-command').textContent);
  assert.match(d.querySelector('#copy-status').textContent, /Copied/);
  w.navigator.clipboard.writeText = async () => { throw Error('Clipboard blocked'); };
  d.querySelector('#copy').click();
  await new Promise(resolve => setTimeout(resolve, 0));
  assert.equal(w.getSelection().toString(), copied);
  assert.match(d.querySelector('#copy-status').textContent, /Command selected/);
  d.querySelector('#demo').click();
  assert.equal(d.querySelector('#demo').disabled, true);
  assert.match(d.querySelector('#trace').textContent, /429/);
  await new Promise(resolve => setTimeout(resolve, 25));
  assert.match(d.querySelector('#trace').textContent, /200/);
  assert.equal(d.querySelector('#demo').disabled, false);
  d.querySelector('#demo').click();
  await new Promise(resolve => setTimeout(resolve, 25));
  assert.equal(d.querySelectorAll('#trace p').length, 3, 'Replay clears the previous trace');
  // Tako Code: downloads point at the five release executables and the swarm demo completes.
  const downloads = [...d.querySelectorAll('.os-links a')].map(a => a.href.split('/').pop());
  assert.deepEqual(downloads, ['tako-code-macos-aarch64', 'tako-code-macos-x86_64', 'tako-code-linux-x86_64', 'tako-code-linux-aarch64', 'tako-code-windows-x86_64.exe']);
  assert.match(d.querySelector('#code').textContent, /free models only/);
  d.querySelector('#swarm-run').click();
  assert.equal(d.querySelector('#swarm-run').disabled, true);
  assert.equal(d.querySelectorAll('#swarm .agent').length, 3);
  await new Promise(resolve => setTimeout(resolve, 25));
  assert.equal((d.querySelector('#swarm').textContent.match(/· done/g) || []).length, 3, 'All three agents finish');
  assert.match(d.querySelector('#swarm').textContent, /tests passed/);
  assert.match(d.querySelector('#swarm').textContent, /Codex.*done/s);
  assert.equal(d.querySelector('#swarm-run').disabled, false);
  d.querySelector('#swarm-run').click();
  await new Promise(resolve => setTimeout(resolve, 25));
  assert.equal(d.querySelectorAll('#swarm .agent').length, 3, 'Replay starts fresh');
  assert.deepEqual(errors, []);
  dom.window.close();
  console.log('Landing checks passed: assets, anchors, copy/fallback, demo completion and replay.');
})().catch(e => { console.error(e); process.exitCode = 1; });
