import { build } from 'esbuild';
await build({entryPoints:['scripts/mcp-analysis.ts'],bundle:true,platform:'node',format:'cjs',outfile:process.argv[2],logLevel:'warning'});
