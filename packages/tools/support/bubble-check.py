import json, os, subprocess, sys
d = sys.argv[1]
tool = sys.argv[2] if len(sys.argv) > 2 else os.path.join(d, '.wire-dump')
names = sorted(f for f in os.listdir(d) if f.endswith('.bin'))
files = [os.path.join(d, n + '.out') if os.path.exists(os.path.join(d, n + '.out')) else os.path.join(d, n) for n in names]
dump = subprocess.run([tool] + files, capture_output=True, text=True).stdout.splitlines()
nodes, children, parent = {}, {}, {}
def setp(nid, props):
    p = nodes.setdefault(nid, {'type': '?', 'p': {}})['p']
    for prop in props:
        if prop[1] == 0:
            p.pop(prop[0], None)
        else:
            p[prop[0]] = prop[2]
for line in dump:
    for op in json.loads(line)['ops']:
        if op[0] == 1:
            nodes[op[1]] = {'type': op[2], 'p': {}}
            setp(op[1], op[3])
        elif op[0] == 2:
            setp(op[1], op[2])
        elif op[0] == 3:
            for c in children.get(op[1], []):
                if parent.get(c) == op[1]:
                    parent.pop(c)
            children[op[1]] = op[2]
            for c in op[2]:
                parent[c] = op[1]
        elif op[0] == 4:
            for c in op[1]:
                nodes.pop(c, None)
                old = parent.pop(c, None)
                if old is not None and c in children.get(old, []):
                    children[old] = [x for x in children[old] if x != c]
def attached(n):
    seen = 0
    while n in parent and seen < 200:
        n = parent[n]; seen += 1
    return n == 1
ident = lambda n: nodes.get(n, {}).get('p', {}).get(9)
bad, good = [], 0
for n, node in nodes.items():
    i = ident(n)
    if isinstance(i, str) and i.startswith('message.content:') and attached(n):
        bg = node['p'].get(90)
        if bg:
            good += 1
        else:
            bad.append((n, i))
print('bubbled', good, 'unbubbled', len(bad))
for n, i in bad[:15]:
    chain = []
    m = n
    for _ in range(6):
        m = parent.get(m)
        if m is None: break
        chain.append(f"{m}:{nodes.get(m,{}).get('type')}:{ident(m)}")
    print(n, i, '<-', ' <- '.join(chain))
def model(paths):
    nodes, children, parent = {}, {}, {}
    def setp(nid, props):
        p = nodes.setdefault(nid, {'type': '?', 'p': {}})['p']
        for prop in props:
            if prop[1] == 0: p.pop(prop[0], None)
            else: p[prop[0]] = prop[2]
    for f in paths:
        line = subprocess.run([tool, f], capture_output=True, text=True).stdout
        for op in json.loads(line)['ops']:
            if op[0] == 1: nodes[op[1]] = {'type': op[2], 'p': {}}; setp(op[1], op[3])
            elif op[0] == 2: setp(op[1], op[2])
            elif op[0] == 3:
                children[op[1]] = op[2]
                for c in op[2]: parent[c] = op[1]
            elif op[0] == 4:
                for c in op[1]: nodes.pop(c, None)
    return nodes
inputs = model([os.path.join(d, n) for n in names])
stray = []
for n, node in nodes.items():
    i = ident(n)
    if not attached(n) or (isinstance(i, str) and i.startswith('message.content:')):
        continue
    before = inputs.get(n, {}).get('p', {}).get(90)
    after = node['p'].get(90)
    if after and after != before and n < 1_000_000:
        stray.append((n, node['type'], i, before, after))
print('stray backgrounds', len(stray))
for s in stray[:10]: print(s)
for n, t, i, b, a in stray[:10]:
    m, chain = n, []
    for _ in range(5):
        m = parent.get(m)
        if m is None: break
        chain.append(f"{m}:{nodes.get(m,{}).get('type')}:{ident(m)}")
    kids = [f"{c}:{nodes.get(c,{}).get('type')}:{ident(c)}" for c in children.get(n, [])][:3]
    print(n, t, 'kids', kids, '<-', ' <- '.join(chain[:4]))
