#!/usr/bin/env python3
"""Black-box CLI tests against an isolated, real Neo4j instance.

Run after cargo build, with PROJMAN_NEO4J_URI pointing at the test instance.
Every run creates a fresh UUID workspace and never touches other workspaces.
"""
import concurrent.futures
import json
import os
from pathlib import Path
import subprocess
import unittest
import uuid
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/debug/projman"
SCHEMA = ROOT / "schemas/examples/research.json"

class CliTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not os.environ.get("PROJMAN_NEO4J_URI"):
            raise RuntimeError("Set PROJMAN_NEO4J_URI to the isolated test database")
        cls.directory = tempfile.TemporaryDirectory(prefix="projman-cli-test-")
        cls.env = dict(os.environ, PROJMAN_WORKSPACE="test-" + str(uuid.uuid4()), XDG_STATE_HOME=cls.directory.name)
        cls.run_cli("workspace", "init")
        cls.run_cli("schema", "publish", "--file", str(SCHEMA), "--expect-schema", "0", "--expect-graph", "0")

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    @classmethod
    def run_cli(cls, *args, data=None, expected=0):
        result = subprocess.run([str(BINARY), "--json", *map(str,args)], env=cls.env,
                                input=None if data is None else json.dumps(data),
                                text=True, capture_output=True, timeout=30)
        if result.returncode != expected:
            raise AssertionError(f"{args}: exit {result.returncode}; stdout={result.stdout}; stderr={result.stderr}")
        envelope = json.loads(result.stdout)
        assert envelope["protocol_version"] == 1
        assert envelope["ok"] == (expected == 0)
        return envelope["data"] if expected == 0 else envelope["error"]

    def state(self):
        return self.run_cli("export")

    def create(self, kind="investigation", props=None, body=""):
        result = self.run_cli("node", "create", "--type", kind, "--stdin", data={"properties":props or {},"body":body})
        return result["result"]["created_nodes"][0]

    def mutation(self, operations, state=None):
        state = state or self.state()
        return {"operation_id":str(uuid.uuid4()),"expected_schema":state["schema_revision"],
                "expected_nodes":{i:n["revision"] for i,n in state["nodes"].items()},
                "action":{"kind":"changes","operations":operations}}

    def apply(self, mutation, expected=0):
        return self.run_cli("change", "apply", "--stdin", data=mutation, expected=expected)

    def test_01_runtime_custom_types_draft_and_roundtrip(self):
        info = self.run_cli("doctor")
        self.assertEqual(info["database"][0]["edition"], "community")
        self.assertEqual(info["database"][0]["versions"][0], "5.26.12")
        types = self.run_cli("type", "list")
        self.assertEqual([t["key"] for t in types["node_types"]], ["investigation","deliverable"])
        node = self.create(props={"tags":["æøå","graph"],"estimate":3})
        draft = self.run_cli("node", "get", node)
        self.assertEqual(draft["missing"], ["summary"])
        self.assertEqual(draft["properties"]["priority"], "normal")
        body = "## Notes\n\nFree text with `code`.\n---\n"
        self.run_cli("node", "update", node, "--expect-revision", 1, "--stdin",
                     data={"set":{"summary":"Unicode æøå"},"body":body})
        saved = self.run_cli("node", "get", node)
        self.assertEqual(saved["body"],body)
        self.assertEqual(saved["missing"],[])
        self.run_cli("node", "update", node, "--expect-revision", 1, "--stdin",data={"set":{"summary":"STALE"}},expected=4)
        self.assertEqual(self.run_cli("node","get",node)["properties"]["summary"],"Unicode æøå")
        self.run_cli("node","create","--type","investigation","--stdin",data={"properties":{"priority":"invalid"}},expected=3)

    def test_02_atomicity_and_receipt_replay(self):
        node = str(uuid.uuid4())
        mutation = self.mutation([{"op":"create_node","id":node,"type_key":"deliverable","properties":{"label":"Receipt"}}])
        first = self.apply(mutation)
        second = self.apply(mutation)
        self.assertEqual(first,second)
        mutation["action"]["operations"][0]["properties"]["label"]="different"
        self.apply(mutation,expected=4)
        bad_id=str(uuid.uuid4())
        mutation=self.mutation([{"op":"create_node","id":bad_id,"type_key":"deliverable"},
                                {"op":"create_node","type_key":"unknown"}])
        self.apply(mutation,expected=6)
        self.run_cli("node","get",bad_id,expected=6)
        preview=self.mutation([{"op":"create_node","id":str(uuid.uuid4()),"type_key":"deliverable"}])
        self.run_cli("change","validate","--stdin",data=preview)
        self.run_cli("node","get",preview["action"]["operations"][0]["id"],expected=6)

    def test_03_outline_typed_suggestions_mentions_and_rename(self):
        a=self.create(props={"summary":"Outline root"})
        b=self.create("deliverable",{"label":"Child"})
        mutation=self.mutation([{"op":"add_link","source":a,"target":b,"type_key":"contains"}])
        receipt=self.apply(mutation)
        self.assertEqual(self.run_cli("outline",a,"--family","breakdown")["children"][0]["id"],b)
        suggestions=self.run_cli("link","suggest","--source",a,"--target",b)["items"]
        self.assertFalse(any(s["type_key"]=="contains" and s["direction"]=="outgoing" for s in suggestions))
        self.apply(self.mutation([{"op":"update_node","id":a,"body":f"[Child](project://{b})"},
                                  {"op":"update_node","id":b,"set":{"label":"Renamed"}}]))
        links=self.run_cli("backlinks",b)
        self.assertEqual(links["edges"][0]["label"],"Part of")
        self.assertEqual(links["mentions"][0]["id"],a)
        self.assertEqual(self.run_cli("outline",a,"--family","breakdown")["children"][0]["title"],"Renamed")
        self.assertEqual(self.run_cli("operation",receipt["operation_id"]),receipt)

    def test_04_competing_cycle_closing_transactions(self):
        a,b,c,d=[self.create() for _ in range(4)]
        self.apply(self.mutation([{"op":"add_link","source":b,"target":c,"type_key":"requires"},
                                  {"op":"add_link","source":d,"target":a,"type_key":"requires"}]))
        state=self.state()
        # Disjoint endpoints: node optimistic revisions alone cannot prevent this cycle.
        first=self.mutation([{"op":"add_link","source":a,"target":b,"type_key":"requires"}],state)
        second=self.mutation([{"op":"add_link","source":c,"target":d,"type_key":"requires"}],state)
        def attempt(m):
            p=subprocess.run([str(BINARY),"--json","change","apply","--stdin"],env=self.env,input=json.dumps(m),text=True,capture_output=True,timeout=30)
            return p.returncode,json.loads(p.stdout)
        with concurrent.futures.ThreadPoolExecutor(2) as pool:
            results=list(pool.map(attempt,[first,second]))
        self.assertEqual(sorted(code for code,_ in results),[0,3],results)
        self.assertTrue(any("Cycle" in r.get("error",{}).get("message","") for _,r in results))

    def test_05_competing_saves(self):
        node=self.create()
        state=self.state()
        mutations=[self.mutation([{"op":"update_node","id":node,"set":{"summary":s}}],state) for s in ["first","second"]]
        def attempt(m):
            p=subprocess.run([str(BINARY),"--json","change","apply","--stdin"],env=self.env,input=json.dumps(m),text=True,capture_output=True,timeout=30)
            return p.returncode
        with concurrent.futures.ThreadPoolExecutor(2) as pool:
            self.assertEqual(sorted(pool.map(attempt,mutations)),[0,4])

    def test_06_stdio_protocol_matches_cli(self):
        request={"protocol_version":1,"id":42,"method":"type.list","params":{}}
        p=subprocess.run([str(BINARY),"serve","--stdio"],env=self.env,input="{bad}\n"+json.dumps(request)+"\n",text=True,capture_output=True,timeout=30)
        self.assertEqual(p.returncode,0,p.stderr)
        responses=[json.loads(line) for line in p.stdout.splitlines()]
        self.assertFalse(responses[0]["ok"])
        self.assertEqual(responses[1]["id"],42)
        self.assertEqual(responses[1]["data"],self.run_cli("type","list"))

    def test_07_durable_recovery_offline_and_conflict(self):
        node=self.create(props={"summary":"Recovery original"})
        doc=self.run_cli("node","get",node,"--document")
        recovery={"id":str(uuid.uuid4()),"workspace":self.env["PROJMAN_WORKSPACE"],"node_id":node,"type_key":"investigation",
                  "expected_schema":1,"expected_revision":1,"text":doc["text"].replace("Recovery original","Recovered edit")}
        saved=self.run_cli("recovery","save","--stdin",data=recovery)
        self.assertEqual(os.stat(saved["path"]).st_mode & 0o777,0o600)
        offline=self.run_cli("--uri","bolt://127.0.0.1:1","recovery","list")
        self.assertIn(recovery["id"],[r["id"] for r in offline["items"]])
        self.run_cli("recovery","restore",recovery["id"])
        self.assertEqual(self.run_cli("node","get",node)["properties"]["summary"],"Recovered edit")
        self.run_cli("recovery","show",recovery["id"],expected=6)
        self.run_cli("recovery","save","--stdin",data=recovery)
        self.run_cli("recovery","restore",recovery["id"],expected=4)
        self.assertEqual(self.run_cli("recovery","show",recovery["id"])["text"],recovery["text"])

    def test_08_proposal_review_dependencies_partial_apply_and_rejection(self):
        root=self.create(props={"summary":"Proposal root"})
        child=str(uuid.uuid4())
        state=self.state()
        draft={"id":str(uuid.uuid4()),"base_schema":state["schema_revision"],"base_graph":state["revision"],
               "expected_nodes":{root:state["nodes"][root]["revision"]},"groups":[
                   {"id":"child","title":"Create deliverable","rationale":"A separate output","operations":[{"op":"create_node","id":child,"type_key":"deliverable","properties":{"label":"Proposed output"}}]},
                   {"id":"link","title":"Attach output","rationale":"Part of this investigation","depends_on":["child"],"operations":[{"op":"add_link","source":root,"target":child,"type_key":"contains"}]},
                   {"id":"note","title":"Add note","rationale":"Optional clarification","operations":[{"op":"update_node","id":root,"body":"Optional proposal note"}]}]}
        submitted=self.run_cli("proposal","submit","--stdin",data=draft)
        digest=submitted["result"]["digest"]
        self.assertEqual(self.state()["revision"],state["revision"])
        self.run_cli("node","get",child,expected=6)
        preview=self.run_cli("proposal","show",draft["id"])
        self.assertEqual(preview["proposal"]["digest"],digest)
        self.assertTrue(any(n["id"]==child for n in preview["nodes"]))
        self.run_cli("proposal","apply",draft["id"],"--digest",digest,"--groups","link",expected=3)
        self.run_cli("proposal","apply",draft["id"],"--digest","wrong","--groups","child,link",expected=4)
        operation=str(uuid.uuid4())
        first=self.run_cli("proposal","apply",draft["id"],"--digest",digest,"--groups","child,link","--operation-id",operation)
        replay=self.run_cli("proposal","apply",draft["id"],"--digest",digest,"--groups","child,link","--operation-id",operation)
        self.assertEqual(first,replay)
        self.assertEqual(self.run_cli("node","get",root)["body"],"")
        self.assertEqual(self.run_cli("outline",root,"--family","breakdown")["children"][0]["id"],child)
        self.run_cli("proposal","apply",draft["id"],"--digest",digest,"--groups","note",expected=4)
        state=self.state()
        draft["id"]=str(uuid.uuid4());draft["base_graph"]=state["revision"]
        draft["expected_nodes"]={root:state["nodes"][root]["revision"]};draft["groups"]=[draft["groups"][2]]
        submitted=self.run_cli("proposal","submit","--stdin",data=draft)
        self.run_cli("proposal","reject",draft["id"],"--digest",submitted["result"]["digest"])
        self.assertEqual(self.state()["revision"],state["revision"])
        self.assertEqual(self.run_cli("node","get",root)["body"],"")
        draft["id"]=str(uuid.uuid4())
        submitted=self.run_cli("proposal","submit","--stdin",data=draft)
        self.create()
        self.run_cli("proposal","apply",draft["id"],"--digest",submitted["result"]["digest"],"--groups","note",expected=4)

    def test_09_ordering_reparent_and_convenience_retries(self):
        parent,other=self.create(),self.create()
        children=[self.create("deliverable",{"label":name}) for name in ["first","second","third"]]
        edge_ids=[]
        for child in children:
            revision=self.state()["revision"];operation=str(uuid.uuid4())
            args=("link","add","--source",parent,"--target",child,"--type","contains","--expect-graph",revision,"--operation-id",operation)
            receipt=self.run_cli(*args)
            self.assertEqual(receipt,self.run_cli(*args))
            edge_ids.append(receipt["result"]["added_edges"][0])
        self.run_cli("link","reorder",parent,"--family","breakdown","--edges",','.join(reversed(edge_ids)),"--expect-graph",self.state()["revision"])
        self.assertEqual([n["id"] for n in self.run_cli("outline",parent,"--family","breakdown")["children"]],list(reversed(children)))
        reparent_args=('link','reparent',edge_ids[0],'--parent',other,'--expect-graph',self.state()['revision'],'--operation-id',str(uuid.uuid4()))
        moved=self.run_cli(*reparent_args)
        self.assertEqual(self.run_cli("outline",other,"--family","breakdown")["children"][0]["id"],children[0])
        self.assertEqual(len(self.run_cli("outline",parent,"--family","breakdown")["children"]),2)
        self.run_cli('link','remove',edge_ids[0],'--expect-graph',self.state()['revision'])
        self.assertEqual(moved,self.run_cli(*reparent_args), 'Retry returns the original receipt even after later edge removal')

    def test_10_custom_scalar_kinds_and_editor_arguments(self):
        workspace='kinds-'+str(uuid.uuid4())
        def run(*args,**kwargs): return self.run_cli('--workspace',workspace,*args,**kwargs)
        run('workspace','init')
        schema={"node_types":[{"key":"reading","name":"Reading","display_property":"name","properties":[
            {"key":"name","type":"string","pattern":"^[A-Z]","min_length":2},
            {"key":"count","type":"integer","min":0,"max":9},
            {"key":"rate","type":"number","min":0},
            {"key":"rates","type":"list","items":"number"},
            {"key":"enabled","type":"boolean"},
            {"key":"when","type":"date"},
            {"key":"checks","type":"list","items":"boolean"}
        ]}],"relationship_types":[]}
        run('schema','publish','--stdin','--expect-schema',0,'--expect-graph',0,data=schema)
        props={"name":"Reading","count":3,"rate":1.25,"rates":[1,2.5,3],"enabled":False,"when":"2024-02-29","checks":[True,False]}
        receipt=run('node','create','--type','reading','--stdin',data={'properties':props})
        node=receipt['result']['created_nodes'][0]
        self.assertEqual(run('node','get',node)['properties'],props)
        for patch in [{'count':10},{'count':1.5},{'when':'2025-02-29'},{'rates':['wrong']},{'name':'lowercase'},{'unknown':1}]:
            run('node','update',node,'--expect-revision',1,'--stdin',data={'set':patch},expected=3)
        run('node','create','--type','reading','--stdin',data={'unexpected':1},expected=3)
        run('node','update',node,'--expect-revision',1,'--stdin',data=[],expected=3)
        editor=Path(self.directory.name)/'editor with spaces.py'
        editor.write_text('import pathlib,sys\np=pathlib.Path(sys.argv[1])\np.write_text(p.read_text().replace("Reading", "Revised"))\n')
        run('node','edit',node,'--editor',f'python3 "{editor}"')
        self.assertEqual(run('node','get',node)['properties']['name'],'Revised')
        run('--non-interactive','node','edit',node,expected=2)

    def test_11_competing_parents(self):
        first,second,child=self.create(),self.create(),self.create('deliverable')
        state=self.state()
        requests=[self.mutation([{'op':'add_link','source':parent,'target':child,'type_key':'contains'}],state) for parent in [first,second]]
        def attempt(request):
            p=subprocess.run([str(BINARY),'--json','change','apply','--stdin'],env=self.env,input=json.dumps(request),text=True,capture_output=True,timeout=30)
            return p.returncode
        with concurrent.futures.ThreadPoolExecutor(2) as pool:
            self.assertEqual(sorted(pool.map(attempt,requests)),[0,4])
        edges=[edge for edge in self.state()['edges'].values() if edge['target']==child and edge['type_key']=='contains']
        self.assertEqual(len(edges),1)

    def test_12_archive_pagination_search_and_relation_first(self):
        parent=self.create(props={'summary':'Searchable '+str(uuid.uuid4())})
        child=self.create('deliverable',{'label':'Archive target'})
        self.assertNotIn('password',self.run_cli('config'))
        page=self.run_cli('node','list','--limit',1)
        self.assertEqual(len(page['items']),1)
        following=self.run_cli('node','list','--limit',1,'--offset',page['next_offset'])
        self.assertNotEqual(page['items'][0]['id'],following['items'][0]['id'])
        self.assertIn(parent,[n['id'] for n in self.run_cli('search','Searchable')['items']])
        allowed=self.run_cli('node','list','--source',parent,'--relation','contains')['items']
        self.assertIn(child,[n['id'] for n in allowed])
        self.run_cli('node','archive',child,'--expect-revision',1)
        self.assertNotIn(child,[n['id'] for n in self.run_cli('node','list')['items']])
        self.assertIn(child,[n['id'] for n in self.run_cli('node','list','--include-archived')['items']])
        self.apply(self.mutation([{'op':'add_link','source':parent,'target':child,'type_key':'contains'}]),expected=3)
        self.run_cli('node','archive',child,'--restore','--expect-revision',2)
        self.apply(self.mutation([{'op':'add_link','source':parent,'target':child,'type_key':'contains'}]))

    def test_13_database_failure_rolls_back_prior_writes(self):
        existing='ffffffff-'+str(uuid.uuid4())[9:]
        self.run_cli('node','create','--type','investigation','--id',existing)
        workspace='rollback-'+str(uuid.uuid4())
        self.run_cli('--workspace',workspace,'workspace','init')
        self.run_cli('--workspace',workspace,'schema','publish','--file',str(SCHEMA),'--expect-schema',0,'--expect-graph',0)
        new_node='00000000-'+str(uuid.uuid4())[9:]
        request={'operation_id':str(uuid.uuid4()),'expected_schema':1,'expected_graph':1,'expected_nodes':{},'action':{'kind':'changes','operations':[
            {'op':'create_node','id':new_node,'type_key':'investigation'},
            {'op':'create_node','id':existing,'type_key':'investigation'}]}}
        # Both nodes validate locally. The first INSERT succeeds, and the second
        # hits the real database's global UUID uniqueness constraint.
        error=self.run_cli('--workspace',workspace,'change','apply','--stdin',data=request,expected=4)
        self.assertIn('Constraint',error['message'])
        self.run_cli('--workspace',workspace,'node','get',new_node,expected=6)
        self.run_cli('--workspace',workspace,'operation',request['operation_id'],expected=6)
        self.assertEqual(self.run_cli('--workspace',workspace,'workspace','show')['graph_revision'],1)
        self.assertEqual(self.run_cli('node','get',existing)['revision'],1)

    def test_99_schema_preview_and_migration(self):
        node=self.create(props={"summary":"Migration"})
        schema=json.loads(SCHEMA.read_text())
        schema["node_types"][0]["properties"].append({"key":"decision","type":"string","required":True})
        preview=self.run_cli("schema","preview","--stdin",data=schema)
        self.assertIn(node,[item.get("node_id") for item in preview["impact"]])
        args=("schema","publish","--expect-schema",1,"--expect-graph",preview["graph_revision"],"--stdin")
        self.run_cli(*args,data=schema,expected=3)
        self.run_cli(*args,"--retain-legacy",data=schema)
        old=self.run_cli("node","get",node)
        self.assertEqual(old["schema_revision"],1)
        self.run_cli("node","update",node,"--expect-revision",old["revision"],"--stdin",data={"set":{"summary":"bad"}},expected=4)
        self.run_cli("schema","migrate",node,"--expect-graph",self.state()["revision"],"--stdin",data={"properties":old["properties"]})
        new=self.run_cli("node","get",node)
        self.assertEqual(new["schema_revision"],2)
        self.assertEqual(new["missing"],["decision"])

if __name__=="__main__":
    unittest.main(verbosity=2)
