"""Serve one step of a memory close-out as a page. The user selects an action for each row and
clicks Done. The app writes the actions to --out and exits. The agent applies the actions, then
serves the next step on the same --port, and the open page reloads into that step.

    python3 -I app.py branch --branch <b> --out <file> [--port N] [--advice <file>] [--print]
    python3 -I app.py main   --branch <b> --out <file> [--port N] [--advice <file>] [--print]

`branch`: the branch's new memories, the branch's proposals, and main's memories.
`main`: main's proposals after the promote, and main's memories.
--advice is the agent's recommendation for each row: {"<id>": {"pick": "<action>", "why": "…"}}.
"""

import argparse
import json
import pathlib
import subprocess
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PAGE = pathlib.Path(__file__).with_name("page.html")

# The actions each list accepts. The page does not post the default action (Keep, Skip).
CHOICES = {"memories": {"retract"}, "overlaps": {"apply", "reject", "drop"}, "main": {"retract"}}


def run(*cmd):
    return subprocess.run(cmd, check=True, capture_output=True, text=True).stdout


def facts(tree):
    """Every fact in a `memories tree --json` node, depth first."""
    yield from tree.get("facts", [])
    for child in tree.get("children", []):
        yield from facts(child)


def tree(branch, *extra):
    out = run("entire", "brain", "memories", "tree", "--branch", branch, "--json",
              "--depth", "3", "--leaves", "100000", *extra)
    return json.loads(out)


def memory(f):
    return {"id": f["id"], "kind": f.get("kind", ""), "path": (f.get("paths") or [""])[0],
            "text": f["text"]}


def overlaps(branch):
    queue = json.loads(run("entire", "brain", "memories", "review", "--branch", branch,
                           "--json")).get("proposals") or []
    by_id = {f["id"]: f for f in facts(tree(branch, "--all"))}
    side = lambda i: memory(by_id.get(i, {"id": i, "text": i}))
    return [{"id": p["candidate_id"], "action": p["action"], "confidence": p.get("confidence"),
             "candidate": side(p["candidate_id"]), "target": side(p["target_id"])}
            for p in queue]


def data_for(step, branch):
    main_all = list(facts(tree("main", "--all")))
    main = sorted((memory(f) for f in main_all if f.get("status") == "active"),
                  key=lambda r: r["path"])
    if step == "main":
        return {"step": "main", "branch": branch, "memories": [], "overlaps": overlaps("main"),
                "main": main}
    # A branch's store starts as a copy of main's, and a fact keeps its id when promoted:
    # the branch's new memories are its own facts that main doesn't have yet. A fact retracted
    # on main counts as reviewed.
    in_main = {f["id"] for f in main_all}
    new = sorted((memory(f) for f in facts(tree(branch))
                  if f.get("branch") == branch and f.get("status") == "active"
                  and f["id"] not in in_main), key=lambda r: r["path"])
    prs = json.loads(run("gh", "pr", "list", "--head", branch, "--state", "all",
                         "--json", "number,state,url"))
    pr = prs[0] if prs else None
    local = subprocess.run(["git", "rev-parse", "--verify", "--quiet", f"refs/heads/{branch}"],
                           capture_output=True).returncode == 0
    current = run("git", "branch", "--show-current").strip()
    return {"step": "branch", "branch": branch, "pr": pr, "memories": new,
            "overlaps": overlaps(branch), "main": main,
            "can_delete": local and current != branch and pr is not None
            and pr["state"] == "MERGED"}


def checked(body, data):
    """The user's actions: {list: {id: action}}, with only the ids and actions this step served."""
    out = {"step": data["step"], "branch": data["branch"]}
    for name, allowed in CHOICES.items():
        picks = body.get(name, {})
        served = {r["id"] for r in data[name]}
        if not isinstance(picks, dict) or not set(picks) <= served \
                or not set(picks.values()) <= allowed:
            raise ValueError(f"{name}: an id or an action that the app did not serve")
        out[name] = picks
    out["delete_local_branch"] = bool(body.get("delete_local_branch")) and data.get("can_delete", False)
    return out


def serve(data, out_path, port):
    body_page = PAGE.read_bytes()
    body_data = json.dumps(data).encode()

    class Handler(BaseHTTPRequestHandler):
        def send(self, code, body, kind):
            self.send_response(code)
            self.send_header("Content-Type", kind)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            if self.path == "/":
                self.send(200, body_page, "text/html; charset=utf-8")
            elif self.path == "/data":
                self.send(200, body_data, "application/json")
            else:
                self.send(404, b"not found", "text/plain")

        def do_POST(self):
            if self.path != "/done":
                return self.send(404, b"not found", "text/plain")
            # Only the served page can post: another site's page cannot send JSON here without
            # a CORS preflight, and the Host check stops DNS rebinding.
            if self.headers.get("Host") != f"127.0.0.1:{server.server_address[1]}":
                return self.send(403, b"wrong host", "text/plain")
            if self.headers.get_content_type() != "application/json":
                return self.send(415, b"send application/json", "text/plain")
            try:
                n = int(self.headers.get("Content-Length", 0))
                body = json.loads(self.rfile.read(n))
                if not isinstance(body, dict):
                    raise ValueError("the body is not a JSON object")
                choices = checked(body, data)
            except ValueError as e:
                return self.send(400, str(e).encode(), "text/plain")
            out_path.parent.mkdir(parents=True, exist_ok=True)
            out_path.write_text(json.dumps(choices, indent=1))
            self.send(200, b"saved", "text/plain")
            threading.Thread(target=server.shutdown).start()

        def log_message(self, *args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    print(f"http://127.0.0.1:{server.server_address[1]}/", flush=True)
    server.serve_forever()
    print(f"saved {out_path}", flush=True)


def main():
    p = argparse.ArgumentParser()
    p.add_argument("step", choices=["branch", "main"])
    p.add_argument("--branch", required=True)
    p.add_argument("--out", required=True, type=pathlib.Path)
    p.add_argument("--port", type=int, default=0, help="serve on the port of the open page (step 2)")
    p.add_argument("--advice", type=pathlib.Path, help="the agent's recommendation for each row")
    p.add_argument("--print", action="store_true", help="print the data and exit")
    a = p.parse_args()
    data = data_for(a.step, a.branch)
    advice = json.loads(a.advice.read_text()) if a.advice else {}
    ids = {r["id"] for name in CHOICES for r in data[name]}
    data["advice"] = {i: v for i, v in advice.items() if i in ids}
    if a.print:
        print(json.dumps(data, indent=1))
    else:
        serve(data, a.out, a.port)


if __name__ == "__main__":
    main()
