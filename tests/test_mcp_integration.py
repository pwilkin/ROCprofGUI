from pathlib import Path
import os,sys,json,time,subprocess,threading,hashlib,urllib.request,urllib.error
from mcp_client import Client
repo=Path(__file__).resolve().parents[1]; root=repo/'.codex/mcp/acceptance';root.mkdir(parents=True,exist_ok=True)
port=18771;token=root/'token';env=dict(os.environ,TMPDIR=str(repo/'.codex/pw'));env.pop('DISPLAY',None);env.pop('WAYLAND_DISPLAY',None)
p=subprocess.Popen([os.environ.get('MCP_TEST_BINARY',str(repo/'target/debug/rocprof-studio')),'--headless','--mcp-port',str(port),'--mcp-token-file',str(token)],cwd=repo,env=env,stdout=(root/'stdout.log').open('w'),stderr=(root/'server.log').open('w'))
checks=[]
try:
 for _ in range(100):
  if p.poll() is not None:raise RuntimeError((root/'server.log').read_text())
  if token.exists():
   try:c=Client(port,token.read_text());break
   except OSError:pass
  time.sleep(.1)
 else:raise RuntimeError('No server')
 checks.append('headless initializes without DISPLAY or WAYLAND_DISPLAY')
 assert len(c.send('tools/list',{})['tools'])>=23
 for h in [{},{'Authorization':'Bearer '+token.read_text(),'Origin':'https://example.com'}]:
  try:urllib.request.urlopen(urllib.request.Request(c.base+'/sse',headers=h));raise AssertionError('Unauthorized access')
  except urllib.error.HTTPError as e:assert e.code in [401,403]
 checks.append('missing bearer and foreign origin rejected')
 legacy=Client(port,token.read_text(),True);assert legacy.tool('workspace')['captures']==[];legacy.close();checks.append('legacy SSE endpoint and message exchange')
 capture=c.tool('capture_open',{'path':str(next((repo/'.codex/kernel-tuning/llama-trace').rglob('*kernel_trace.csv')))})
 cid=capture['id'];rows=c.tool('capture_query',{'captureId':cid,'dataset':'dispatches','query':'scale_f32','limit':1});assert rows['rows'];dispatch=rows['rows'][0]
 kernels=c.tool('capture_analyze',{'captureId':cid,'analysis':'kernels','query':'scale_f32'});assert kernels['total']>0
 reference=c.tool('reference_query',{'architecture':'rdna4','query':'V_WMMA','limit':2});assert reference['instructions']['total']>0
 checks.append('real trace import, paged kernel query, shared GUI analysis and ISA reference')
 out=root/str(p.pid);out.mkdir()
 c.tool('capture_analyze',{'captureId':cid,'analysis':'timeline','output':str(out/'timeline.html'),'timePages':2})
 assert (out/'timeline.html').read_text().endswith('</body></html>')
 c.tool('capture_analyze',{'captureId':cid,'analysis':'diagnostics','output':str(out/'diagnostics.json')})
 assert json.loads((out/'diagnostics.json').read_text())['kind']=='rocprof-studio-diagnostics'
 tensor=c.tool('capture_analyze',{'captureId':cid,'analysis':'tensor','definitions':'Y := X + 1','raw':{'X':{'shape':[1,1,1,1],'values':[2]}}});assert tensor['Y']['values']==[3]
 c.tool('definition_file',{'op':'write','path':str(out/'counter.def'),'text':'Y := X + 1'})
 assert c.tool('definition_file',{'op':'read','path':str(out/'counter.def')})['text']=='Y := X + 1'
 c.tool('project_save',{'path':str(out/'project.rocprof-project')})
 opened=c.tool('project_open',{'path':str(out/'project.rocprof-project')});cid=opened['captures'][0]['id']
 c.tool('capture_export',{'captureId':cid,'path':str(out/'capture.rocprof-capture')})
 assert (out/'capture.rocprof-capture').exists()
 c.tool('plot_export',{'path':str(out/'plot.svg'),'format':'svg','text':'<svg xmlns="http://www.w3.org/2000/svg"></svg>'})
 bad=c.tool('capture_query',{'captureId':cid,'dataset':'dispatches','limit':-1},raw=True);assert bad['error']['code']==-32602
 checks.append('shared timeline and diagnostic exports, tensor formulas, definition files, project round-trip, portable capture, SVG and schema validation')
 tuning=c.tool('tuning_open',{'captureId':cid,'dispatchId':dispatch['id']});tid=tuning['session']['id']
 state=c.tool('tuning_action',{'id':tid,'request':{'op':'search','config':{'sourceTree':'/devel/tools/llama.cpp'}}})
 index=next(i for i,x in enumerate(state['session']['candidates']) if x['path'].endswith('/ggml/src/ggml-cuda/scale.cu'))
 selected=c.tool('tuning_action',{'id':tid,'request':{'op':'select','index':index}});assert selected['session']['code']
 checks.append('MCP source search and selection use the shared tuning backend')
 other=Client(port,token.read_text()); outcome={}
 thread=threading.Thread(target=lambda:outcome.update(value=c.tool('profile_run',{'cwd':str(repo),'argv':['/usr/bin/python3','-c','import subprocess,time; subprocess.Popen(["/usr/bin/sleep","120"]); time.sleep(120)']},raw=True)));thread.start()
 time.sleep(2)
 busy=other.tool('workspace',raw=True);assert busy['error']['code']==-32001,busy
 other.notify('notifications/cancelled',{'requestId':c.number});time.sleep(.2);assert thread.is_alive()
 c.notify('notifications/cancelled',{'requestId':c.number});thread.join(15);assert not thread.is_alive();assert outcome['value']['error']['code']==-32800,outcome
 checks.append('concurrent actions rejected; cancellation scoped to requesting session; worker group stopped')
 assert c.tool('workspace')['control']['active']['tool']=='workspace'
 if '--full' in sys.argv:
  captured=c.tool('tuning_action',{'id':tid,'request':{'op':'capture'}});assert captured['session']['captured']
  baseline=c.tool('tuning_action',{'id':tid,'request':{'op':'run','iterations':20,'variant':False,'autoCalibrate':True,'autoCounters':True,'validate':True}})
  assert baseline['session']['runs'][-1]['validation']['status']=='bitwise match'
  code=baseline['session']['code'];edited=code.replace('for (int64_t i = tid;', '#pragma unroll 2\n    for (int64_t i = tid;')
  assert edited!=code
  variant=c.tool('tuning_action',{'id':tid,'request':{'op':'run','iterations':20,'variant':True,'code':edited,'autoCalibrate':True,'autoCounters':True,'validate':True}})
  assert variant['session']['runs'][-1]['validation']['status']=='bitwise match'
  source=Path(variant['session']['source']['path']);before=hashlib.sha256(source.read_bytes()).hexdigest()
  saved=c.tool('tuning_action',{'id':tid,'request':{'op':'save','code':code}})
  assert hashlib.sha256(source.read_bytes()).hexdigest()==before and saved['session']['lastSave']['backup']
  (root/'tuning-result.json').write_text(json.dumps(saved,indent=2));checks.append('real application capture, calibrated baseline, edited replay, bitwise match, deltas and original-source save through MCP')
 c.tool('tuning_close',{'id':tid});other.close()
 shutdown={};pidfile=out/'pids.json'
 code='import subprocess,time,json,os; child=subprocess.Popen(["/usr/bin/sleep","120"]); open('+repr(str(pidfile))+',"w").write(json.dumps([os.getpid(),child.pid])); time.sleep(120)'
 thread=threading.Thread(target=lambda:shutdown.update(value=c.tool('profile_run',{'cwd':str(repo),'argv':['/usr/bin/python3','-c',code]},raw=True)));thread.start()
 for _ in range(100):
  if pidfile.exists():break
  time.sleep(.1)
 assert pidfile.exists()
 owned=json.loads(pidfile.read_text());p.terminate();p.wait(timeout=15);thread.join(10)
 assert not thread.is_alive() and shutdown['value']['error']['code']==-32800,shutdown
 for pid in owned:
  stat=Path(f'/proc/{pid}/stat')
  assert not stat.exists() or stat.read_text().split()[2]=='Z',f'Worker survived shutdown: {pid}'
 checks.append('SIGTERM cancels active MCP request and kills application subprocess descendants')
 (root/'results.json').write_text(json.dumps({'checks':checks,'tuningId':tid},indent=2));print(json.dumps(checks),flush=True)
finally:
 p.terminate()
 try:p.wait(timeout=15)
 except subprocess.TimeoutExpired:p.kill();p.wait()
