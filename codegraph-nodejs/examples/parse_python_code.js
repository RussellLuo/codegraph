import * as codegraph from '../index.js';
import * as path from 'path';

const REPO_DIR = path.join(path.dirname(path.dirname(import.meta.dirname)), 'examples', 'python');
const CODE_DIR = path.join(REPO_DIR, 'd.py');

const graph = new codegraph.CodeGraph("./graph/test_db");

graph.index(REPO_DIR, CODE_DIR);

const nodes = graph.query("MATCH (n) RETURN *");
for (let n of nodes ) {
  console.log(`${n.name}(${n.startLine}:${n.endLine})`);
  console.log(n.code);
}

graph.clean(true);
