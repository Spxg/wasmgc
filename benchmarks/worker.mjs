import { B, do_not_optimize } from './mitata/main.mjs';
import { createSuite } from './suite.mjs';
import { summarize } from './stats.mjs';

self.onmessage = async ({ data: { name, implementation } }) => {
  try {
    let gc = {};
    let wbg = {};
    if (implementation === 'wasmgc') {
      const bytes = await (await fetch('./wasmgc.wasm')).arrayBuffer();
      const { instance } = await WebAssembly.instantiate(bytes);
      gc = instance.exports;
    } else {
      wbg = await import('./wasm-bindgen/wasm_bindgen.js');
      await wbg.default();
    }

    const test = createSuite(gc, wbg, do_not_optimize).cases.find(test => test.name === name);
    const trial = await new B(`${name}/${implementation}`, test.callbacks[implementation])
      .gc(false)
      .run(true);
    self.postMessage({ result: summarize(trial.runs[0].stats, test.operations) });
  } catch (error) {
    self.postMessage({ error: error.stack || String(error) });
  }
};
