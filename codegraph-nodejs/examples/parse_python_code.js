import * as codegraph from '../index.js';
import * as path from 'path';

const REPO_DIR = path.join(path.dirname(path.dirname(import.meta.dirname)), 'examples', 'python');

const config = {
  // Only include "d.py"
  ignorePatterns: [
    "*",
    "!d.py",
  ],
};
const parser = new codegraph.Parser(config);
const { nodes, relationships } = parser.parse(REPO_DIR);

for (let n of nodes ) {
  console.log(`${n.name}(${n.startLine}:${n.endLine})`);
  console.log(n.code);
}

for (let r of relationships ) {
  console.log(`relationship: ${r.type}\nfrom: ${r.from.name}\nto: ${r.to.name}`);
}
