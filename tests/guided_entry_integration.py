#!/usr/bin/env python3
"""Exercise user-facing guided entry commands against an isolated Neo4j workspace."""
import json,os,pathlib,subprocess,tempfile,uuid
root=pathlib.Path(__file__).resolve().parents[1]
if not os.environ.get('PROJMAN_NEO4J_URI'):
    raise RuntimeError('Set PROJMAN_NEO4J_URI to an isolated test database')
with tempfile.TemporaryDirectory(prefix='projman-entry-') as tmp:
    env=dict(os.environ,PROJMAN_TEST_ROOT=str(root),PROJMAN_WORKSPACE='entry-'+str(uuid.uuid4()),XDG_STATE_HOME=tmp,NVIM_LOG_FILE=tmp+'/nvim.log')
    def cli(*args,data=None):
        p=subprocess.run([str(root/'target/debug/projman'),'--json',*args],env=env,input=json.dumps(data) if data is not None else None,text=True,capture_output=True,timeout=30)
        assert p.returncode==0,(p.stdout,p.stderr)
        return json.loads(p.stdout)['data']
    cli('workspace','init')
    schema={'node_types':[
      {'key':'task','name':'Task','display_property':'title','properties':[
        {'key':'title','label':'Title','type':'string','required':True,'min_length':3},
        {'key':'status','label':'Status','type':'enum','required':True,'choices':['todo','in progress','done'],'default':'todo'},
        {'key':'ready','label':'Ready','type':'boolean','required':True,'default':False},
        {'key':'due','label':'Due date','type':'date','required':True},
        {'key':'hours','label':'Hours','type':'integer','min':0,'max':8},
        {'key':'tags','label':'Tags','type':'list','items':'string'}]},
      {'key':'note','name':'Note','display_property':'name','properties':[{'key':'name','type':'string','required':True}]}]}
    cli('schema','publish','--stdin','--expect-schema','0','--expect-graph','0',data=schema)
    subprocess.run(['nvim','--headless','-u','NONE','-l',str(root/'tests/nvim_guided_entry.lua')],env=env,check=True,timeout=60)
    state=cli('export');nodes=list(state['nodes'].values())
    main=next(n for n in nodes if n['properties'].get('title')=='Guided title')
    assert main['properties']['status']=='done' and main['properties']['ready'] is False
    assert main['properties']['hours']==4 and main['properties']['tags']==['API','UI']
    assert main['body']=='Notes typed before opening properties.'
    assert len(nodes)==2,'Cancelling entry and opening a raw draft must not create nodes'
    assert any(n['missing']==['title','due'] for n in nodes)
    print('Guided entry integration passed: creation, validation retries, defaults, notes, typed property editing, cancellation and drafts')
