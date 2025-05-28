import * as codegraph from '../index.js';
import * as path from 'path';

const REPO_DIR = path.join(path.dirname(path.dirname(import.meta.dirname)), 'examples', 'python');
const CODE_DIR = path.join(REPO_DIR, 'd.py');

const parser = new codegraph.Parser();
const nodes = parser.parse(REPO_DIR, CODE_DIR);

for (let n of nodes ) {
  console.log(`${n.name}(${n.startLine}:${n.endLine})`);
  console.log(n.code);
}