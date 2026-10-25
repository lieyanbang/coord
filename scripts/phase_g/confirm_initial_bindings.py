"""Confirm exact public binding reconciliation; never broadens authorization."""
import json
from pathlib import Path
base=Path('C:/ProgramData/coord/g7')
pointer=base/'private/initial-operation.json'
state=json.loads(pointer.read_text())
binding=globals()['_BINDING']
assert state['state']=='WINDOWS_BOUND' and state['binding']==binding
assert json.loads((base/'relay-private/identity.json').read_text())['bindings'][-1]==binding
state['state']='ALL_HOPS_BOUND'
pointer.write_text(json.dumps(state,indent=2))
print(json.dumps({'state':'PASS','batch_id':binding['batch_id'],'binding':binding}))
