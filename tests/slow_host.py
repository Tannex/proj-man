#!/usr/bin/env python3
"""Test proxy that delays mutation delivery to exercise editor disconnection."""
import json, signal, subprocess, sys, time

def terminate(_signum, _frame):
    raise SystemExit(0)

signal.signal(signal.SIGTERM, terminate)
child=subprocess.Popen([sys.argv[1],*sys.argv[2:]],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
try:
    for line in sys.stdin:
        request=json.loads(line)
        if request.get('method')=='change.apply':
            time.sleep(0.3)
        child.stdin.write(line)
        child.stdin.flush()
        response=child.stdout.readline()
        if not response:
            break
        middle=len(response)//2
        sys.stdout.write(response[:middle])
        sys.stdout.flush()
        time.sleep(0.01)
        sys.stdout.write(response[middle:])
        sys.stdout.flush()
finally:
    child.terminate()
    child.wait(timeout=5)
