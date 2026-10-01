#!/usr/bin/env python3
"""Regression for plain-text header writes, in standalone Neovim and lazy.nvim."""
import json, os, pathlib, subprocess, tempfile, uuid
root=pathlib.Path(__file__).resolve().parents[1]
if not os.environ.get('PROJMAN_NEO4J_URI'):
    raise RuntimeError('Set PROJMAN_NEO4J_URI to an isolated test database')
manager=os.environ.get('PROJMAN_TEST_LAZY_ROOT')
with tempfile.TemporaryDirectory(prefix='projman-save-test-') as tmp:
    env=dict(os.environ,PROJMAN_TEST_ROOT=str(root),XDG_STATE_HOME=tmp+'/state',XDG_CACHE_HOME=tmp+'/cache',XDG_DATA_HOME=tmp+'/data',XDG_CONFIG_HOME=tmp+'/config',NVIM_LOG_FILE=tmp+'/nvim.log')
    def cli(*args,data=None):
        p=subprocess.run([str(root/'target/debug/projman'),'--json',*map(str,args)],env=env,input=json.dumps(data) if data is not None else None,text=True,capture_output=True,timeout=30)
        assert p.returncode==0,(args,p.stdout,p.stderr)
        return json.loads(p.stdout)['data']
    for mode in ('standalone','lazy') if manager else ('standalone',):
        if mode=='lazy': env['PROJMAN_TEST_LAZY_ROOT']=manager
        else: env.pop('PROJMAN_TEST_LAZY_ROOT',None)
        env['PROJMAN_WORKSPACE']='save-'+str(uuid.uuid4())
        cli('workspace','init')
        schema={'node_types':[{'key':'task','name':'Task','display_property':'name','properties':[
            {'key':'name','type':'string','required':True},
            {'key':'status','type':'enum','choices':['todo','in progress','done']},
            {'key':'due','type':'date'}, {'key':'hours','type':'integer'},
            {'key':'ready','type':'boolean'}, {'key':'tags','type':'list','items':'string'}]}]}
        cli('schema','publish','--stdin','--expect-schema',0,'--expect-graph',0,data=schema)
        receipt=cli('node','create','--type','task','--stdin',data={'properties':{'name':'Before'}})
        env['PROJMAN_TEST_NODE']=receipt['result']['created_nodes'][0]
        subprocess.run(['nvim','--headless','-u','NONE','-l',str(root/'tests/nvim_save.lua')],env=env,check=True,timeout=45)
        node=cli('node','get',env['PROJMAN_TEST_NODE'])
        assert node['properties']['name']=='Final quoted value' and node['properties']['hours']==6
        assert node['body']=='## Notes\n\nBody text with --- markers.\n'
        # Recovery and CLI editor paths share the same schema-aware parser.
        record={'id':str(uuid.uuid4()),'workspace':env['PROJMAN_WORKSPACE'],'node_id':node['id'],'type_key':node['type_key'],
                'expected_schema':1,'expected_revision':node['revision'],'text':'--- projman\nname: Restored plain text\n---\nRecovery body\n'}
        cli('recovery','save','--stdin',data=record)
        cli('recovery','restore',record['id'])
        assert cli('node','get',node['id'])['properties']['name']=='Restored plain text'
        editor=pathlib.Path(tmp)/'edit.py'
        editor.write_text('import pathlib,sys\np=pathlib.Path(sys.argv[1])\ntext=p.read_text().splitlines(keepends=True)\ntext[1]="name: CLI edited text\\n"\np.write_text("".join(text))\n')
        cli('node','edit',node['id'],'--editor',f'python3 "{editor}"')
        assert cli('node','get',node['id'])['properties']['name']=='CLI edited text'
        print(mode+': persisted node changes confirmed independently through CLI')
