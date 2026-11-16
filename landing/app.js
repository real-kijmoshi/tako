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
// Illustrative swarm: the planner splits a task, three agents finish, then merge, test and review.
const swarmRun = document.getElementById('swarm-run');
swarmRun.addEventListener('click', () => {
  const swarm = document.getElementById('swarm');
  const step = window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : 650;
  const agents = [['Claude Code', 'headers middleware'], ['Codex', 'config option'], ['OpenCode', 'integration tests']];
  const row = (mark, cls, engine, text) => `<p class="agent"><span class="${cls}">${mark}</span><span class="engine">${engine}</span><span>${text}</span></p>`;
  swarmRun.disabled = true;
  swarmRun.textContent = 'Swarm running…';
  swarm.innerHTML = '<p><span class="blue">→</span> Planner split the task into 3 parts</p>' + agents.map(([e, t]) => row('◌', 'dim', e, t + ' <span class="dim">· working</span>')).join('');
  agents.forEach(([e, t], i) => setTimeout(() => {
    swarm.querySelectorAll('.agent')[i].outerHTML = row('✓', 'good', e, t + ' <span class="good">· done</span>');
  }, step * (i + 1)));
  setTimeout(() => {
    swarm.innerHTML += '<p><span class="good">✓</span> Merged on a review branch · tests passed</p><p><span class="blue">→</span> Ready for review. Your branch is unchanged.</p>';
    document.getElementById('swarm-note').textContent = 'Three agents, one review branch, free models only.';
    swarmRun.textContent = 'Run it again ↗';
    swarmRun.disabled = false;
  }, step * 4);
});
