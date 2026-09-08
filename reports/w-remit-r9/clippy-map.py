import re,subprocess,sys
BASE="a6217328"
log=open(sys.argv[1]).read()
files=subprocess.run(["git","diff","--name-only",BASE,"HEAD"],capture_output=True,text=True).stdout.split()
added={}
for f in files:
    d=subprocess.run(["git","diff","-U0",BASE,"HEAD","--",f],capture_output=True,text=True).stdout
    s=set()
    for m in re.finditer(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@",d,flags=re.M):
        st=int(m.group(1)); c=int(m.group(2)) if m.group(2) is not None else 1
        s.update(range(st,st+c))
    added[f]=s
blocks=re.split(r"\n(?=(?:warning|error)(?:\[|:))",log)
mine=[];others=0
for b in blocks:
    m=re.search(r"--> ([^:\n]+):(\d+):(\d+)",b)
    if not m: continue
    path,line=m.group(1),int(m.group(2))
    head=b.splitlines()[0]
    if path in added and line in added[path]:
        mine.append((path,line,head))
    else: others+=1
print("diagnostics on lines added a6217328..HEAD:",len(mine)," elsewhere:",others)
for p,l,h in mine: print(f"  {p}:{l}  {h}")
