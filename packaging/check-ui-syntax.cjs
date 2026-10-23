// Syntax-only regression check: a parse error disables all toolbar IPC handlers.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const root = path.resolve(__dirname, '..', 'rust-browser');
for (const name of ['chrome.html', 'overlay.html', 'new_tab.html']) {
  const html = fs.readFileSync(path.join(root, 'ui', name), 'utf8');
  for (const [index, match] of [...html.matchAll(/<script\b[^>]*>([\s\S]*?)<\/script>/gi)].entries()) {
    new vm.Script(match[1], {filename: `${name}:script-${index}`});
  }
}
for (const name of ['worker.js', 'content.js']) {
  new vm.Script(fs.readFileSync(path.join(root, 'fixtures', 'mv3-probe', name), 'utf8'), {filename: name});
}
console.log('Internal UI and MV3 fixture scripts parse successfully.');
