// brain-graph.mjs - rebuild the served graph: every repo's graphify graph merged into the vault's
// graphify-out/merged-graph.json, what .mcp.json serves. Runtime: bun --bun.
// The vault itself is not in it since vault audit Stage 3: `knowledge graph` answers vault links.
import { rebuildGraph } from "./brain-lib.mjs";

rebuildGraph();
