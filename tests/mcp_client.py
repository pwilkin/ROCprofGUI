import json, urllib.request, urllib.error, threading, queue

class Client:
    def __init__(self, port, token, legacy=False):
        self.base=f'http://127.0.0.1:{port}'; self.token=token; self.sid=None; self.number=0; self.legacy=legacy; self.responses=queue.Queue(); self.lock=threading.Lock()
        self.endpoint='/mcp'; self.stream=None
        if legacy:
            self.stream=urllib.request.urlopen(urllib.request.Request(self.base+'/sse',headers=self.headers()),timeout=180)
            for line in self.stream:
                if line.startswith(b'data: '): self.endpoint=line.decode().strip()[6:]; break
            def consume():
                try:
                    for line in self.stream:
                        if line.startswith(b'data: '): self.responses.put(json.loads(line[6:]))
                except (OSError,ValueError):pass
            threading.Thread(target=consume,daemon=True).start()
        self.send('initialize',{'protocolVersion':'2024-11-05' if legacy else '2025-06-18','capabilities':{},'clientInfo':{'name':'rocprof-studio-acceptance','version':'1'}})
        self.notify('notifications/initialized',{})
    def headers(self):
        h={'Authorization':'Bearer '+self.token,'Content-Type':'application/json','Accept':'application/json, text/event-stream','MCP-Protocol-Version':'2025-06-18'}
        if self.sid:h['Mcp-Session-Id']=self.sid
        return h
    def post(self,body):
        r=urllib.request.urlopen(urllib.request.Request(self.base+self.endpoint,data=json.dumps(body).encode(),headers=self.headers()),timeout=700)
        if r.headers.get('Mcp-Session-Id'):self.sid=r.headers['Mcp-Session-Id']
        return r
    def notify(self,method,params):
        with self.post({'jsonrpc':'2.0','method':method,'params':params}) as r:assert r.status==202
    def send(self,method,params,raw=False):
        with self.lock:self.number+=1;number=self.number
        with self.post({'jsonrpc':'2.0','id':number,'method':method,'params':params}) as r:
            if self.legacy:value=self.responses.get(timeout=700)
            else:
                value=None
                for line in r:
                    if line.startswith(b'data: '):value=json.loads(line[6:])
        assert value['id']==number,value
        if raw:return value
        if 'error' in value:raise RuntimeError(value['error'])
        return value['result']
    def tool(self,name,args=None,raw=False):
        result=self.send('tools/call',{'name':name,'arguments':args or {}},raw=raw)
        if raw:return result
        if result.get('isError'):raise RuntimeError(result['content'])
        return json.loads(result['content'][0]['text'])
    def close(self):
        if self.stream:self.stream.close()
        elif self.sid:
            with urllib.request.urlopen(urllib.request.Request(self.base+'/mcp',method='DELETE',headers=self.headers())):pass
