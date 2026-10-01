#!/usr/bin/env python3
"""CLI and Neovim explorer checks against an isolated Neo4j workspace."""
import json, os, pathlib, subprocess, tempfile, uuid
root=pathlib.Path(__file__).resolve().parents[1]
if not os.environ.get('PROJMAN_NEO4J_URI'):
    raise RuntimeError('Set PROJMAN_NEO4J_URI to an isolated test database')
with tempfile.TemporaryDirectory(prefix='projman-explorer-') as temporary:
    env=dict(os.environ,PROJMAN_WORKSPACE='explorer-'+str(uuid.uuid4()),PROJMAN_TEST_ROOT=str(root),XDG_STATE_HOME=temporary,NVIM_LOG_FILE=temporary+'/nvim.log')
    binary=str(root/'target/debug/projman')
    def cli(*args,data=None,code=0):
        p=subprocess.run([binary,'--json',*map(str,args)],env=env,input=json.dumps(data) if data is not None else None,text=True,capture_output=True,timeout=30)
        assert p.returncode==code,(args,p.stdout,p.stderr)
        payload=json.loads(p.stdout)
        return payload['data'] if code==0 else payload['error']
    cli('workspace','init')
    schema={'node_types':[{'key':'topic','name':'Topic','display_property':'title','properties':[{'key':'title','type':'string','required':True}]}],
            'relationship_types':[{'key':'relates','name':'Relates to','inverse_name':'Related from','sources':['topic'],'targets':['topic'],'family':'references','allow_self':True}]}
    cli('schema','publish','--stdin','--expect-schema',0,'--expect-graph',0,data=schema)
    labels={'root':'Root project','a':'A branch','b':'B branch','c':'C branch','d':'Shared deliverable','archived':'Archived item','needle':'ZZZ needle root'}
    labels.update({f'filler{i}':f'Filler {i:03}' for i in range(65)})
    ids={key:str(uuid.uuid4()) for key in labels}
    operations=[{'op':'create_node','id':ids[key],'type_key':'topic','properties':{'title':title}} for key,title in labels.items()]
    for source,target in [('root','a'),('root','b'),('a','c'),('c','d'),('b','d'),('d','root'),('a','a'),('root','archived')]:
        operations.append({'op':'add_link','source':ids[source],'target':ids[target],'type_key':'relates'})
    operations.append({'op':'update_node','id':ids['archived'],'archived':True})
    cli('change','apply','--stdin',data={'operation_id':str(uuid.uuid4()),'expected_schema':1,'expected_nodes':{},'action':{'kind':'changes','operations':operations}})
    baseline=cli('export')
    tree=cli('explore',ids['root'])
    assert [n for n in tree['nodes'] if n['id']==ids['d']][0]['parent']==ids['b']
    assert len(tree['nodes'])==5
    assert any(r['from']==ids['c'] and r['to']==ids['d'] for r in tree['references'])
    assert any(r['from']==ids['d'] and r['to']==ids['root'] for r in tree['references'])
    both=cli('explore',ids['root'],'--direction','both')
    assert [n for n in both['nodes'] if n['id']==ids['d']][0]['depth']==1
    assert cli('explore',ids['root'],'--max-depth',0)['truncated']['depth']
    assert cli('explore',ids['root'],'--max-nodes',2)['truncated']['nodes']
    assert len(cli('explore',ids['root'],'--include-archived')['nodes'])==6
    cli('explore',ids['root'],'--max-depth',33,code=3)
    first=cli('node','pick','--limit',50)
    assert first['total']==71 and first['next_offset']==50
    assert ids['needle'] not in [n['id'] for n in first['items']]
    match=cli('node','pick','NEEDLE topic')
    assert len(match['items'])==1 and match['items'][0]['id']==ids['needle']
    fixture=pathlib.Path(temporary)/'ids.json';fixture.write_text(json.dumps(ids));env['PROJMAN_TEST_IDS']=str(fixture)
    subprocess.run(['nvim','--headless','-u','NONE','-l',str(root/'tests/nvim_explorer_integration.lua')],env=env,check=True,timeout=45)
    after=cli('export')
    assert after==baseline,'Explorer or picker unexpectedly mutated the graph'
    print('Explorer integration passed: CLI/RPC BFS, cycles, archives, bounds, picker pagination, sidebar navigation and unchanged graph')
