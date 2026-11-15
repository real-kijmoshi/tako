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
  assert.deepEqual(errors, []);
  dom.window.close();
  console.log('Landing checks passed: assets, anchors, copy/fallback, demo completion and replay.');
})().catch(e => { console.error(e); process.exitCode = 1; });
