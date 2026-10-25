$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$base='C:\ProgramData\coord\g7'
$batch='d2313c78-22da-4cb8-bcd7-ece9897e0db1'
$key=[IO.File]::ReadAllText((Join-Path $base 'daemon-private/management.key')).Trim()
function Rpc([string]$Method,$Params){
 $r=Invoke-RestMethod 'http://127.0.0.1:7777/' -Method Post -ContentType application/json -Headers @{Authorization=('Bearer '+$key)} -Body (@{jsonrpc='2.0';id=1;method=$Method;params=$Params}|ConvertTo-Json -Depth 20 -Compress)
 if($r.error){throw ('Read-only RPC failed: '+$Method)};return $r.result
}
$progress=Rpc 'batches/status' @{batch_id=$batch}
$actions=Rpc 'actions/status' @{batch_id=$batch}
if($progress.total -ne 4 -or $progress.states.completed -ne 4 -or $progress.active -ne 0){throw 'Browser Canary requires four completed tasks.'}
if(@($actions.actions).Count -ne 5 -or @($actions.actions|Where-Object state -ne 'succeeded').Count){throw 'Four publish actions plus aggregate must succeed.'}
$inspect=@'
import collections,hashlib,json,pathlib,sqlite3
db=sqlite3.connect('file:C:/ProgramData/coord/data/coord.db?mode=ro',uri=True)
db.row_factory=sqlite3.Row
batch='d2313c78-22da-4cb8-bcd7-ece9897e0db1'
tasks=list(db.execute('SELECT id,state,claimed_by,attempt_no,claimed_at,completed_at,result FROM tasks WHERE batch_id=? ORDER BY completed_at,id',(batch,)))
assert len(tasks)==4 and all(t['state']=='completed' and t['attempt_no']==1 for t in tasks)
assert collections.Counter(t['claimed_by'] for t in tasks)=={'emote-0000':2,'emote-0001':2}
proof=[];root=pathlib.Path('C:/ProgramData/coord/g7/browser/business/results')
for t in tasks:
    p=list(db.execute('SELECT agent_id,attempt_no,sequence,checkpoint,progress_at,last_extended_sequence FROM task_progress WHERE task_id=?',(t['id'],)))
    e=list(db.execute('SELECT progress_sequence,lease_until FROM task_progress_extend_receipts WHERE task_id=?',(t['id'],)))
    receipts=list(db.execute('SELECT operation,agent_id FROM task_receipts WHERE task_id=?',(t['id'],)))
    assert len(p)==len(e)==1 and p[0]['sequence']==p[0]['last_extended_sequence']==e[0]['progress_sequence']==1
    assert p[0]['agent_id']==t['claimed_by'] and p[0]['attempt_no']==1
    result=json.loads(t['result']);published=json.loads((root/(result['group_id']+'.json')).read_text(encoding='utf-8'))
    assert published==result and result['agent_id']==t['claimed_by'] and result['attempt_no']==1
    assert result['characters']==[] and len(result['uncertain'])==1
    proof.append({**{k:t[k] for k in ('id','state','claimed_by','attempt_no','claimed_at','completed_at')},'progress':dict(p[0]),'renewal':dict(e[0]),'receipt_metadata':[dict(x) for x in receipts],'group_id':result['group_id'],'published_matches_result':True})
files=[]
for name in ['00.json','01.json','02.json','03.json','index.json','review_queue/candidates.json']:
    p=root/name;b=p.read_bytes();json.loads(b);files.append({'path':name,'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()})
index=json.loads((root/'index.json').read_bytes());review=json.loads((root/'review_queue/candidates.json').read_bytes())
assert index['state']=='STRUCTURALLY_COMPLETE' and index['expected_groups']==index['available_results']==4 and index['missing_groups']==[]
assert {e['group_id'] for e in index['results']}=={'00','01','02','03'}
for e in index['results']:
    assert e['sha256']==hashlib.sha256((root/e['path']).read_bytes()).hexdigest()
assert review['candidate_count']==0 and review['candidates']==[] and review['missing_groups']==[]
assert db.execute('PRAGMA quick_check').fetchall()[0][0]=='ok'
print(json.dumps({'state':'PASS','tasks':proof,'files':files,'index':index,'review_queue':review,'tokens_exported':False,'quick_check':'ok'}))
'@
$raw=& 'C:\Users\lieyanbang\AppData\Local\Programs\Python\Python314\python.exe' -c $inspect
if($LASTEXITCODE -ne 0){throw 'Browser task/renewal/publication evidence failed.'}
@{state='PASS';observed_at=[DateTime]::UtcNow.ToString('o');batch_id=$batch;progress=$progress;actions=$actions;database=($raw|ConvertFrom-Json);mutation_performed=$false}|ConvertTo-Json -Depth 25
