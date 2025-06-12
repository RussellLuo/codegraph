import * as codegraph from '../index.js';
import * as path from 'path';

const REPO_DIR = path.join(path.dirname(path.dirname(import.meta.dirname)), 'examples', 'go', 'demo');
const DB_DIR = path.join(REPO_DIR, "kuzu_db");

const config = {
  ignorePatterns: [
    "*",
    "!main.go",
    "!types.go",
  ],
};

(async function() {
  const graph = new codegraph.CodeGraph(DB_DIR, REPO_DIR, config);
  await graph.index(REPO_DIR, false);
  
  const MAIN_GO = path.join(REPO_DIR, "main.go");
  const snippets = graph.getFuncParamTypes(MAIN_GO, 37);
  for (let s of snippets) {
    console.log(`--> ${s.path}:${s.startLine}:${s.endLine}\n${s.content}`);
  }
  
  graph.clean(true);
})();
