# Round 3: map `cargo fmt --all -- --check` blocks onto the lines this delivery added (a6217328..HEAD).
import re,subprocess,sys
BASE="a6217328"
log=open(sys.argv[1]).read()
files=[f for f in subprocess.run(["git","diff","--name-only",BASE,"HEAD"],capture_output=True,text=True).stdout.split() if f.endswith(".rs")]
blocks=re.split(r"^Diff in ", log, flags=re.M)[1:]
fmt={}
for b in blocks:
    head,_,body=b.partition("\n")
    m=re.match(r"(.*?):(\d+):",head)
    path=m.group(1).split("w-seller-fee-stage2a-remit/")[1]; line=int(m.group(2))
    n=len([l for l in body.splitlines() if l and l[0] in "+-"])
    fmt.setdefault(path,[]).append((line,line+max(n,1)))
added=0; total_over=0; on_added=0
for f in files:
    d=subprocess.run(["git","diff","-U0",BASE,"HEAD","--",f],capture_output=True,text=True).stdout
    hunks=[]; mine=set()
    for m in re.finditer(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@",d,flags=re.M):
        s=int(m.group(1)); c=int(m.group(2)) if m.group(2) is not None else 1
        if c>0: hunks.append((s,s+c-1)); mine.update(range(s,s+c))
    a=sum(1 for l in d.splitlines() if l.startswith("+") and not l.startswith("+++"))
    added+=a
    over=[(a_,b_) for (a_,b_) in fmt.get(f,[]) for (s,e) in hunks if a_<=e and b_>=s]
    # a block whose starting line is itself an added line
    starts_on_added=[(a_,b_) for (a_,b_) in fmt.get(f,[]) if a_ in mine]
    total_over+=len(over); on_added+=len(starts_on_added)
    print(f"{f}: added={a} my-hunks={len(hunks)} fmt-diffs-in-file={len(fmt.get(f,[]))} OVERLAPS={over} STARTS-ON-ADDED-LINE={starts_on_added}")
print("Rust added lines",BASE+"..HEAD:",added," fmt blocks total:",len(blocks)," overlapping my hunks:",total_over," starting on an added line:",on_added)
