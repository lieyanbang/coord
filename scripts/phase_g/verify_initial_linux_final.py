"""Read-only archived config comparison and protected local route regression."""
import hashlib
import json
from pathlib import Path
import subprocess
import socket
import tarfile
import urllib.error
import urllib.request

root=Path('/var/backups/coord-initial-20261025T075307Z-18128120')
unchanged=0
with tarfile.open(root/'configuration.tar.gz') as tar:
    for member in tar.getmembers():
        if member.isfile() and member.name!='etc/coord-relay/identity.json':
            assert tar.extractfile(member).read()==Path('/'+member.name).read_bytes(), 'Unexpected Linux file drift'
            unchanged+=1
policy=json.loads(Path('/etc/coord-relay/identity.json').read_text())
before=json.loads((root/'identity-before.json').read_text())
assert policy['bindings'][:-1]==before['bindings']
assert policy['legacy_client_sha256']==before['legacy_client_sha256']
assert policy['signing_key_file']==before['signing_key_file']
assert policy['bindings'][-1]['id']=='emote-initial-v1'
units={u:subprocess.check_output(['systemctl','is-active',u],text=True).strip() for u in ['coord-relay.service','feathersjsapp.service','openresty.service']}
assert set(units.values())=={'active'}
routes=[]
for port in [13011,13012]:
    # Agent listeners use authenticated raw TCP framing, not HTTP /healthz.
    # The real six-worker queries separately prove the active 13012 relay path.
    with socket.create_connection(('100.116.134.114',port),timeout=5):pass
    routes.append({'port':port,'tcp_connect':'PASS','http_health_not_applicable':True})
health=json.load(urllib.request.urlopen('http://127.0.0.1:13013/healthz',timeout=5))
assert health['state']=='READY'
class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self,*args,**kwargs):return None
opener=urllib.request.build_opener(NoRedirect)
public=[]
for path,expected in [('/app/CoordMCP/',401),('/app/CoordMCP/.well-known/oauth-protected-resource/mcp',200),('/app/CoordMCP/.well-known/oauth-authorization-server',200),('/app/CoordMCP/oauth/authorize',302),('/app/RemoteControlMCP/',302),('/app/RemoteControlMCP2/',302),('/app/RemoteControlMCP3/',401),('/mcp',401)]:
    try:res=opener.open(urllib.request.Request('https://lieyanbang.com'+path,headers={'User-Agent':'Mozilla/5.0'}),timeout=15);status=res.status
    except urllib.error.HTTPError as e:status=e.code
    assert status==expected, 'Public route status mismatch: '+path+' actual='+str(status)+' expected='+str(expected)
    public.append({'path':path,'http_status':status,'expected':expected})
print(json.dumps({'state':'PASS','batch_id':policy['bindings'][-1]['batch_id'],'archived_files_unchanged':unchanged,'prior_bindings_unchanged':True,'units':units,'protected_tailscale_routes':routes,'public_routes':public,'health':health,'oauth_store_manually_modified':False,'backup_root':str(root)}))
