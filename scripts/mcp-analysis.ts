import { readFileSync } from 'node:fs';
import { timelineDocument } from '../src/timelineExport';
import { dispatchMarks, globalMarks, defaultGlobalLayers } from '../src/globalTimeline';
import { occupancy, kernelStats } from '../src/analysis';
import { activityCost, hardwareSummary } from '../src/summary';
import { sourceHierarchy } from '../src/sourceHierarchy';
import { markerTree } from '../src/markerAnalysis';
import { scalarDefinition, scalarSeries } from '../src/scalarCounters';
import { diagnosticManifest } from '../src/diagnostics';
import { Tensor, tensorDefinitions, withTensorBudget } from '../src/tensor';
const {capture:c,args:a}=JSON.parse(readFileSync(process.argv[2],'utf8'));
async function main(){
let result;
switch(a.analysis){
 case 'timeline': {
   const marks=a.timeline==='global'?globalMarks(c,defaultGlobalLayers):dispatchMarks(c);
   const blob=await timelineDocument({label:c.name,unit:c.unit,bounds:a.range||[c.start,c.end],timePages:a.timePages||1,lanesPerPage:a.lanesPerPage||12,stacked:true,load:async range=>marks.filter(m=>m.start<range[1]&&m.end>range[0]),signal:new AbortController().signal});
   result={html:await blob.text()};break;
 }
 case 'diagnostics':result=await diagnosticManifest({capture:c,workspace:a.workspace||{},sources:{},definitions:{version:1,selected:'',files:[]},hiddenPending:false,wave:null,runtime:{environment:'MCP',backendVersion:'0.1.0'}});break;
 case 'kernels':result=kernelStats(c);break;
 case 'occupancy':result=occupancy(c,a.se,a.kernel);break;
 case 'activity':result=activityCost(c.instructions);break;
 case 'hardware':result=hardwareSummary(c.architecture,c.counters);break;
 case 'hotspots':result=[...c.instructions].sort((x,y)=>(y[a.metric||'latency']||0)-(x[a.metric||'latency']||0));break;
 case 'source':result=sourceHierarchy(c.instructions,a.metric||'latency');break;
 case 'markers':result=markerTree(c.markers,a.range||[c.start,c.end]);break;
 case 'scalar':result=scalarSeries(scalarDefinition(a.name||'MCP counter',a.formula,c.counters),c.counters);break;
 case 'tensor':result=withTensorBudget(8_000_000,()=>{
   const raw=Object.fromEntries(Object.entries(a.raw||{}).map(([name,v]:[string,any])=>[name,new Tensor(v.shape,v.values)]));
   const defs=tensorDefinitions(a.definitions,raw);return defs;
 });break;
 default:throw Error('Unknown analysis');
}
process.stdout.write(JSON.stringify(result,(_k,v)=>v instanceof Map?Object.fromEntries(v):v));

}
main().catch(error=>{console.error(String(error));process.exitCode=1;});
