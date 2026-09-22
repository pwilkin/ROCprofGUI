from pathlib import Path
import asyncio,json,os,subprocess,time
import httpx2
from mcp import ClientSession
from mcp.client.sse import sse_client
from mcp.client.streamable_http import streamable_http_client
repo=Path(__file__).resolve().parents[1];root=repo/'.codex/mcp/sdk';root.mkdir(exist_ok=True)
token=root/'token';port=18774
binary=Path(os.environ.get('MCP_TEST_BINARY',str(repo/'target/debug/rocprof-studio')))
env=dict(os.environ,TMPDIR=str(repo/'.codex/pw'));env.pop('DISPLAY',None);env.pop('WAYLAND_DISPLAY',None)
p=subprocess.Popen([str(binary),'--headless','--mcp-port',str(port),'--mcp-token-file',str(token)],cwd=repo,env=env,stdout=(root/'stdout.log').open('w'),stderr=(root/'server.log').open('w'))
async def check():
 headers={'Authorization':'Bearer '+token.read_text()}
 results=[]
 async with httpx2.AsyncClient(headers=headers,timeout=60) as http:
  async with streamable_http_client(f'http://127.0.0.1:{port}/mcp',http_client=http) as streams:
   async with ClientSession(*streams[:2]) as session:
    init=await session.initialize();tools=await session.list_tools();result=await session.call_tool('workspace',{})
    assert not result.is_error
    (root.parent/'tools.json').write_text(json.dumps(tools.model_dump(mode='json',by_alias=True),indent=2))
    results.append({'transport':'streamable HTTP','version':init.protocol_version,'tools':len(tools.tools)})
 async with sse_client(f'http://127.0.0.1:{port}/sse',headers=headers) as streams:
  async with ClientSession(*streams[:2]) as session:
   init=await session.initialize();tools=await session.list_tools();result=await session.call_tool('reference_query',{'architecture':'rdna4','query':'V_WMMA','limit':1})
   assert not result.is_error
   results.append({'transport':'legacy SSE','version':init.protocol_version,'tools':len(tools.tools)})
 (root/'results.json').write_text(json.dumps(results,indent=2));print(json.dumps(results))
try:
 for _ in range(100):
  if p.poll() is not None:raise RuntimeError((root/'server.log').read_text())
  if token.exists() and 'listening' in (root/'server.log').read_text():break
  time.sleep(.1)
 asyncio.run(check())
finally:
 p.terminate();p.wait(timeout=15)
