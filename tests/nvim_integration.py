#!/usr/bin/env python3
"""Exercise the optional Neovim client against the same real Neo4j CLI."""
import json, os, pathlib, subprocess, tempfile, uuid
root=pathlib.Path(__file__).resolve().parents[1]
if not os.environ.get('PROJMAN_NEO4J_URI'):
    raise RuntimeError('Set PROJMAN_NEO4J_URI to the isolated test instance')
with tempfile.TemporaryDirectory(prefix='projman-nvim-test-') as temporary:
    env=dict(os.environ,PROJMAN_WORKSPACE='nvim-'+str(uuid.uuid4()),XDG_STATE_HOME=temporary,
             PROJMAN_TEST_ROOT=str(root),NVIM_LOG_FILE=temporary+'/nvim.log')
    binary=str(root/'target/debug/projman')
    def cli(*args,data=None):
        p=subprocess.run([binary,'--json',*args],env=env,input=None if data is None else json.dumps(data),text=True,capture_output=True,timeout=20)
        if p.returncode: raise AssertionError(p.stdout+p.stderr)
        return json.loads(p.stdout)['data']
    cli('workspace','init')
    cli('schema','publish','--file',str(root/'schemas/examples/research.json'),'--expect-schema','0','--expect-graph','0')
    receipt=cli('node','create','--type','investigation','--stdin',data={'properties':{'summary':'Editor initial'}})
    env['PROJMAN_TEST_NODE']=receipt['result']['created_nodes'][0]
    subprocess.run(['nvim','--headless','-u','NONE','-l',str(root/'tests/nvim_integration.lua')],env=env,check=True,timeout=60)
    subprocess.run(['nvim','--headless','-u','NONE','-l',str(root/'tests/nvim_quit.lua')],env=env,check=True,timeout=30)
    assert cli('node','get',env['PROJMAN_TEST_NODE'])['properties']['summary']=='Pending quit save', 'Quit lost an unacknowledged save'
    subprocess.run(['nvim','--headless','-u','NONE','-l',str(root/'tests/nvim_failures.lua')],env=env,check=True,timeout=30)
    assert cli('node','get',env['PROJMAN_TEST_NODE'])['properties']['summary']=='Survives host failure'
    subprocess.run(['nvim','--headless','-u','NONE','-l',str(root/'tests/nvim_timeouts.lua')],env=env,check=True,timeout=30)
    assert cli('node','get',env['PROJMAN_TEST_NODE'])['properties']['summary']=='Unknown outcome recovered once'
