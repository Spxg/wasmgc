import { exampleFor } from './examples.mjs';
import './prism/core.js';
import './prism/rust.js';

globalThis.Prism.manual = true;

const implementationLabels = {
  wasmgc: 'wasmgc',
  'wasm-bindgen': 'wasm-bindgen',
};
const status = document.querySelector('#status');
const errorOutput = document.querySelector('#error');
const runAll = document.querySelector('#run-all');
const rows = [];
const results = new Map([...document.querySelectorAll('section[data-group]')]
  .map(section => [section.dataset.group, section.querySelector('tbody')]));
const nextTask = () => new Promise(resolve => setTimeout(resolve, 0));
let queue = Promise.resolve();
let pending = 0;

document.querySelector('#browser').textContent = navigator.userAgent;
runAll.addEventListener('click', () => rows.forEach(runBenchmark));

async function bytes(path) {
  const response = await fetch(path);
  if (!response.ok) throw new Error(`Could not load ${path}: HTTP ${response.status}`);
  return response.arrayBuffer();
}

function showError(error) {
  status.textContent = 'Benchmark failed.';
  errorOutput.textContent = error.stack || String(error);
  errorOutput.hidden = false;
}

function measure(name, implementation) {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL('./worker.mjs', import.meta.url), { type: 'module' });
    worker.onmessage = ({ data }) => {
      worker.terminate();
      if (data.error) reject(new Error(data.error));
      else resolve(data.result);
    };
    worker.onerror = event => {
      worker.terminate();
      reject(new Error(event.message));
    };
    worker.postMessage({ name, implementation });
  });
}

function createExample(test, example) {
  const row = document.createElement('tr');
  row.className = 'example-row';
  row.hidden = true;
  const cell = document.createElement('td');
  cell.colSpan = Object.keys(test.callbacks).length + 2;
  const panel = document.createElement('div');
  panel.id = `${test.name}-example`;
  panel.className = 'example-panel';
  const description = document.createElement('p');
  description.textContent = example.description;
  panel.append(description);
  if (example.note) {
    const note = document.createElement('p');
    note.className = 'example-note';
    note.textContent = example.note;
    panel.append(note);
  }
  const content = document.createElement('div');
  panel.append(content);
  cell.append(panel);
  row.append(cell);

  function codePanel(title, source) {
    const column = document.createElement('div');
    column.className = 'example-implementation';
    const heading = document.createElement('h3');
    heading.textContent = title;
    column.append(heading);
    const pre = document.createElement('pre');
    const code = document.createElement('code');
    code.className = 'language-rust';
    code.textContent = source.trim();
    pre.append(code);
    globalThis.Prism.highlightElement(code);
    column.append(pre);
    return column;
  }

  function render() {
    const columns = document.createElement('div');
    columns.className = 'example-columns';
    for (const implementation of ['wasmgc', 'wasm-bindgen']) {
      columns.append(codePanel(implementationLabels[implementation], example.implementations[implementation]));
    }
    content.append(columns);
  }

  function alignCodeHeights() {
    const blocks = [...content.querySelectorAll('pre')];
    for (const block of blocks) block.style.height = '';
    const height = Math.max(...blocks.map(block => block.getBoundingClientRect().height));
    for (const block of blocks) block.style.height = `${height}px`;
  }

  let contentWidth;
  const observer = new ResizeObserver(([entry]) => {
    if (row.hidden || entry.contentRect.width === contentWidth) return;
    contentWidth = entry.contentRect.width;
    requestAnimationFrame(() => {
      if (!row.hidden) alignCodeHeights();
    });
  });
  observer.observe(content);

  const button = document.createElement('a');
  button.href = '#';
  button.className = 'example-toggle';
  button.textContent = '(?)';
  button.setAttribute('aria-expanded', 'false');
  button.setAttribute('aria-controls', panel.id);
  button.setAttribute('aria-label', `Show Rust implementations for ${test.name}`);
  button.addEventListener('click', event => {
    event.preventDefault();
    if (!content.hasChildNodes()) render();
    row.hidden = !row.hidden;
    if (!row.hidden) alignCodeHeights();
    button.setAttribute('aria-expanded', String(!row.hidden));
    button.setAttribute('aria-label', `${row.hidden ? 'Show' : 'Hide'} Rust implementations for ${test.name}`);
  });
  return { row, button };
}

function addRow(test) {
  const example = exampleFor(test);
  const disclosure = createExample(test, example);
  const tr = document.createElement('tr');
  tr.dataset.benchmark = test.name;
  const heading = document.createElement('th');
  heading.scope = 'row';
  const name = document.createElement('code');
  name.textContent = test.name;
  heading.append(name, ' ', disclosure.button);
  tr.append(heading);

  const cells = {};
  for (const implementation of Object.keys(test.callbacks)) {
    const cell = document.createElement('td');
    cell.dataset.implementation = implementation;
    cell.dataset.state = 'idle';
    cell.textContent = '—';
    cells[implementation] = cell;
    tr.append(cell);
  }

  const action = document.createElement('td');
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'benchmark-run';
  button.textContent = 'Run';
  button.setAttribute('aria-label', `Run ${test.name}`);
  const row = { test, tr, button, cells, values: {}, runs: 0 };
  rows.push(row);
  tr.dataset.state = 'idle';
  button.addEventListener('click', () => runBenchmark(row));
  action.append(button);
  tr.append(action);
  results.get(test.group).append(tr, disclosure.row);
}

function formatResult(result) {
  const ns = result.meanNs.toLocaleString('en-US', {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  });
  return `${ns} ns ±${result.rme.toFixed(2)}%`;
}

function highlightFastest(row) {
  const complete = Object.keys(row.values).length === Object.keys(row.cells).length;
  const fastest = Math.min(...Object.values(row.values).map(result => result.meanNs));
  for (const [implementation, cell] of Object.entries(row.cells)) {
    cell.classList.toggle('fastest', complete && row.values[implementation]?.meanNs === fastest);
  }
}

function runBenchmark(row) {
  if (row.button.disabled) return;
  const { test } = row;
  row.button.disabled = true;
  row.button.textContent = 'Queued';
  row.tr.dataset.state = 'queued';
  row.values = {};
  for (const cell of Object.values(row.cells)) {
    cell.textContent = '—';
    cell.classList.remove('fastest');
    cell.removeAttribute('title');
    for (const key of ['ns', 'margin', 'rme', 'samples']) delete cell.dataset[key];
    cell.dataset.state = 'queued';
  }
  errorOutput.hidden = true;
  pending++;
  runAll.disabled = true;

  async function execute() {
    row.tr.dataset.state = 'running';
    row.button.textContent = 'Running…';
    let failed = false;
    try {
      const entries = Object.keys(test.callbacks);
      const offset = row.runs++ % entries.length;
      const ordered = [...entries.slice(offset), ...entries.slice(0, offset)];
      for (const implementation of ordered) {
        const cell = row.cells[implementation];
        cell.dataset.state = 'running';
        cell.textContent = 'executing ...';
        status.textContent = `Running ${test.name} with ${implementationLabels[implementation]}…`;
        await nextTask();

        try {
          const result = await measure(test.name, implementation);
          cell.textContent = formatResult(result);
          cell.title = `${result.meanNs} ns/operation; ±${result.marginNs} ns at 95% confidence; ${result.sampleCount} samples`;
          cell.dataset.rme = result.rme;
          cell.dataset.samples = result.sampleCount;
          cell.dataset.ns = result.meanNs;
          cell.dataset.margin = result.marginNs;
          cell.dataset.state = 'done';
          row.values[implementation] = result;
        } catch (error) {
          failed = true;
          cell.dataset.state = 'error';
          cell.textContent = 'error';
          cell.title = error.message || String(error);
          errorOutput.textContent = `${test.name}/${implementation}: ${error.stack || String(error)}`;
          errorOutput.hidden = false;
        }
      }
      highlightFastest(row);
    } catch (error) {
      failed = true;
      showError(error);
    } finally {
      row.tr.dataset.state = failed ? 'error' : 'done';
      row.button.disabled = false;
      row.button.textContent = 'Run';
      pending--;
      runAll.disabled = pending > 0;
      status.textContent = pending ? `${pending} pending benchmark${pending === 1 ? '' : 's'}.`
        : failed ? `Finished ${test.name} with an error.` : 'Ready.';
    }
  }

  queue = queue.then(execute, execute);
}

async function initialize() {
  // Remove the service worker installed by earlier benchmark builds.
  try {
    const registration = await navigator.serviceWorker?.getRegistration(new URL('./', import.meta.url));
    if (registration?.active?.scriptURL === new URL('./coi-serviceworker.js', import.meta.url).href) {
      await registration.unregister();
    }
  } catch {}

  const modules = await Promise.all([
    import('./suite.mjs'),
    import('./wasm-bindgen/wasm_bindgen.js'),
    bytes('./wasmgc.wasm'),
    bytes('./wasm-bindgen/wasm_bindgen_bg.wasm'),
  ]);
  const [suite, bindgen, gcBytes, bindgenBytes] = modules;
  bindgen.initSync({ module: bindgenBytes });
  const { instance } = await WebAssembly.instantiate(gcBytes);

  status.textContent = 'Checking results…';
  const tests = suite.createSuite(instance.exports, bindgen, () => {});
  await tests.check();
  for (const test of tests.cases) addRow(test);
  runAll.disabled = false;
  status.textContent = `Ready. ${tests.cases.length} benchmarks checked.`;
}

window.benchmarkReady = initialize();
window.benchmarkReady.catch(showError);
