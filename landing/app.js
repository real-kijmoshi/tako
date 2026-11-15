'use strict';
const copy = document.getElementById('copy');
copy.addEventListener('click', async () => {
  const command = document.getElementById('install-command');
  const status = document.getElementById('copy-status');
  try {
    await navigator.clipboard.writeText(command.textContent);
    status.textContent = 'Copied. Paste into your terminal to install Tako.';
    copy.textContent = 'Copied!';
  } catch {
    const range = document.createRange();
    range.selectNodeContents(command);
    const selection = window.getSelection();
    selection.removeAllRanges();
    selection.addRange(range);
    status.textContent = 'Command selected. Press Ctrl+C or ⌘C to copy.';
  }
});
const demo = document.getElementById('demo');
demo.addEventListener('click', () => {
  const trace = document.getElementById('trace');
  demo.disabled = true;
  demo.textContent = 'Trying the next provider…';
  trace.innerHTML = '<p><span class="warn">↳</span> Provider A <span class="warn">429 · rate limited</span></p><p><span class="blue">→</span> Trying Provider B…</p>';
  setTimeout(() => {
    trace.innerHTML += '<p><span class="good">✓</span> Provider B <span class="good">200 · response received</span></p>';
    document.getElementById('demo-note').textContent = 'Same request. Same endpoint. Next provider.';
    demo.textContent = 'Replay fallback ↗';
    demo.disabled = false;
  }, window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : 900);
});
