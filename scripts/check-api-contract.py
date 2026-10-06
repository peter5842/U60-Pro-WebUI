#!/usr/bin/env python3
"""Assert the agent, the dashboard and the mock agent agree on the API surface.

The dashboard was originally built against `web-app/tools/mock_agent.py` rather
than the real agent, and the two silently drifted: `/api/dashboard`'s `speed`
block and `/api/system/top` both shipped shapes the UI could not read. This
check closes the loop on the *route* half of that problem — payload shapes are
pinned by the `#[cfg(test)]` key-set assertions in `agent/src/system.rs` and the
dashboard's mapper tests.

Routes are compared as **HTTP method + path**: a GET binding for a PUT-only
route is as broken at runtime as a missing route.

Fails if:
  - the dashboard calls an endpoint the agent does not serve  (broken at runtime)
  - the agent serves an endpoint nothing calls                (dead surface)
  - the mock is missing an endpoint the dashboard reads       (broken demos)
  - the mock serves an endpoint the agent does not            (stale fixture)
  - a dashboard source mentions an `/api/...` path in a form the extractor
    does not understand                                       (would be skipped)

Run from anywhere:  python3 scripts/check-api-contract.py
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SERVER_RS = ROOT / "agent/src/server.rs"
API_TS = ROOT / "web-app/src/data/api.ts"
CLIENT_TS = ROOT / "web-app/src/data/client.ts"
MOCK_PY = ROOT / "web-app/tools/mock_agent.py"

Route = tuple[str, str]  # (METHOD, /api/path)

# The mock answers any unlisted PUT/DELETE/POST with `{"ok": true, "data": {}}`,
# which is an adequate stub for fire-and-forget mutations. It only needs a real
# fixture where the dashboard reads a payload back: every GET, plus these POSTs.
MOCK_PAYLOAD_POSTS = {
    ("POST", "/api/sms/list"),
    ("POST", "/api/at/send"),
    ("POST", "/api/system/kill-bloat"),
    ("POST", "/api/proxy/service"),
    ("POST", "/api/proxy/subscriptions/update"),
    ("POST", "/api/proxy/delay"),
}

AGENT_ROUTE_RE = re.compile(r'\(&Method::(\w+),\s*"(/api/[^"]+)"\)')

# Every `/api/...` string literal in a dashboard source.
API_LITERAL_RE = re.compile(r'''(['"`])(/api/[a-zA-Z0-9/_-]+)\1''')
# The binding forms the dashboard uses. Each must name its method explicitly.
BINDING_RES = (
    # get('/api/x') / post('/api/x', …) / put('/api/x', …)
    (re.compile(r'''\b(get|post|put)\(\s*(['"`])(/api/[a-zA-Z0-9/_-]+)\2'''), None),
    # req('DELETE', '/api/x', …)
    (re.compile(r'''\breq\(\s*['"](GET|POST|PUT|DELETE|PATCH)['"]\s*,\s*(['"`])(/api/[a-zA-Z0-9/_-]+)\2'''), None),
    # readCsv('/api/x') is a GET
    (re.compile(r'''\breadCsv\(\s*(['"`])(/api/[a-zA-Z0-9/_-]+)\1'''), "GET"),
)
# A literal compared against a variable (e.g. `path !== '/api/auth/login'`) is
# not a call site.
COMPARISON_RE = re.compile(r'''[!=]==?\s*(['"`])(/api/[a-zA-Z0-9/_-]+)\1''')

MOCK_TABLE_RE = r"^ROUTES_{method} = \{{(.*?)^\}}"
MOCK_KEY_RE = re.compile(r'''^\s*["'](/api/[a-zA-Z0-9/_-]+)["']\s*:''', re.M)
MOCK_DELETE_RE = re.compile(r'''def do_DELETE\(self\):(.*?)(?=\n    def |\Z)''', re.S)
MOCK_PATH_EQ_RE = re.compile(r'''path == ["'](/api/[a-zA-Z0-9/_-]+)["']''')


def read(path: Path) -> str:
    if not path.exists():
        sys.exit(f"missing file: {path.relative_to(ROOT)}")
    return path.read_text()


def agent_routes(server_rs: str) -> set[Route]:
    """(METHOD, path) pairs from the route table."""
    return {(m.upper(), p) for m, p in AGENT_ROUTE_RE.findall(server_rs)}


def dashboard_routes(source: str) -> tuple[set[Route], set[str]]:
    """Bindings in one dashboard source, plus `/api/` literals no binding explains."""
    routes: set[Route] = set()
    spans: list[tuple[int, int]] = []
    for regex, fixed_method in BINDING_RES:
        for m in regex.finditer(source):
            groups = m.groups()
            method = fixed_method or groups[0].upper()
            routes.add((method, groups[-1]))
            spans.append(m.span())
    for m in COMPARISON_RE.finditer(source):
        spans.append(m.span())
    unexplained = {
        m.group(2)
        for m in API_LITERAL_RE.finditer(source)
        if not any(start <= m.start() and m.end() <= end for start, end in spans)
    }
    return routes, unexplained


def mock_routes(mock_py: str) -> set[Route]:
    routes: set[Route] = set()
    for method in ("GET", "POST", "PUT"):
        m = re.search(MOCK_TABLE_RE.format(method=method), mock_py, re.S | re.M)
        if m:
            routes |= {(method, p) for p in MOCK_KEY_RE.findall(m.group(1))}
    m = MOCK_DELETE_RE.search(mock_py)
    if m:
        routes |= {("DELETE", p) for p in MOCK_PATH_EQ_RE.findall(m.group(1))}
    return routes


def fmt(route: Route) -> str:
    return f"{route[0]:<6} {route[1]}"


def report(title: str, items: set, hint: str) -> bool:
    if not items:
        return True
    print(f"\n  {title}")
    for item in sorted(items):
        print(f"    - {fmt(item) if isinstance(item, tuple) else item}")
    print(f"    -> {hint}")
    return False


def check(server_rs: str, dashboard_sources: list[str], mock_py: str) -> bool:
    agent = agent_routes(server_rs)
    dash: set[Route] = set()
    unexplained: set[str] = set()
    for source in dashboard_sources:
        routes, loose = dashboard_routes(source)
        dash |= routes
        unexplained |= loose
    mock = mock_routes(mock_py)
    # Every GET the dashboard reads, plus the POSTs whose payload it reads back.
    needs_fixture = {r for r in dash if r[0] == "GET"} | (MOCK_PAYLOAD_POSTS & dash)

    print(
        f"agent serves {len(agent)} routes over {len({p for _, p in agent})} paths; "
        f"dashboard binds {len(dash)}; mock serves {len(mock)}"
    )

    ok = True
    ok &= report(
        "Dashboard mentions /api/ paths outside a recognised binding:",
        unexplained,
        "use get/post/put/req(METHOD, …)/readCsv so the method is checkable, "
        "or teach scripts/check-api-contract.py the new form",
    )
    ok &= report(
        "Dashboard calls endpoints the agent does not serve (method + path):",
        dash - agent,
        "these fail at runtime — add the route, fix the method, or drop the call",
    )
    ok &= report(
        "Agent serves endpoints nothing calls:",
        agent - dash,
        "dead surface — delete it or wire it into the dashboard",
    )
    ok &= report(
        "Mock lacks a fixture for endpoints the dashboard reads:",
        needs_fixture - mock,
        "local demos will render empty — add a fixture to mock_agent.py",
    )
    ok &= report(
        "Mock serves endpoints the agent does not (method + path):",
        mock - agent,
        "stale fixture — fix or remove it in mock_agent.py",
    )
    return ok


def main() -> int:
    ok = check(
        read(SERVER_RS),
        [read(API_TS), read(CLIENT_TS)],
        read(MOCK_PY),
    )
    print("\nOK: agent, dashboard and mock agree." if ok else "\nFAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
