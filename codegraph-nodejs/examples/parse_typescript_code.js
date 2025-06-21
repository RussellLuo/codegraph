import * as codegraph from '../index.js';
import * as path from 'path';

const REPO_DIR = path.join(path.dirname(path.dirname(import.meta.dirname)), 'examples', 'typescript');
const DB_DIR = path.join(REPO_DIR, "kuzu_db");

const config = {
  ignorePatterns: [
    "*",
    "!*.ts",
  ],
};

codegraph.initLogger(codegraph.LogLevel.Info);

const graph = new codegraph.CodeGraph(DB_DIR, REPO_DIR, config);
graph.clean(true);
graph.index(REPO_DIR, false);

const MAIN_TS = path.join(REPO_DIR, "main.ts");
const snippets = graph.getFuncParamTypes(MAIN_TS, 25);
for (let s of snippets) {
  console.log(`--> ${s.path}:${s.startLine}:${s.endLine}\n${s.content}`);
}

graph.clean(true);
