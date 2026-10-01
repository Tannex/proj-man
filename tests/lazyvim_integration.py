#!/usr/bin/env python3
"""Test the local spec using installed lazy.nvim/which-key, with isolated state."""
import os, pathlib, subprocess, tempfile
root=pathlib.Path(__file__).resolve().parents[1]
data=pathlib.Path(os.environ.get('XDG_DATA_HOME',str(pathlib.Path.home()/'.local/share')))
lazy_root=pathlib.Path(os.environ.get('PROJMAN_TEST_LAZY_ROOT',str(data/'nvim/lazy')))
for plugin in ('lazy.nvim','which-key.nvim'):
    if not (lazy_root/plugin).is_dir():
        raise SystemExit('Set PROJMAN_TEST_LAZY_ROOT to your installed lazy.nvim/which-key plugin directory')
with tempfile.TemporaryDirectory(prefix='projman-lazyvim-') as temporary:
    env=dict(os.environ,PROJMAN_TEST_ROOT=str(root),PROJMAN_TEST_LAZY_ROOT=str(lazy_root),
             XDG_STATE_HOME=temporary+'/state',XDG_CACHE_HOME=temporary+'/cache',
             XDG_DATA_HOME=temporary+'/data',XDG_CONFIG_HOME=temporary+'/config',NVIM_LOG_FILE=temporary+'/nvim.log')
    subprocess.run(['nvim','--headless','-u','NONE','-l',str(root/'tests/nvim_lazyvim.lua')],env=env,check=True,timeout=30)
