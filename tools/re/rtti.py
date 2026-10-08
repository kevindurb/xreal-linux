import sys
from elftools.elf.elffile import ELFFile
path=sys.argv[1]; names=sys.argv[2:]
data=open(path,"rb").read(); e=ELFFile(open(path,"rb"))
segs=[(s["p_vaddr"],s["p_filesz"],s["p_offset"]) for s in e.iter_segments() if s["p_type"]=="PT_LOAD"]
def off2va(off):
    for v,sz,o in segs:
        if o<=off<o+sz: return v+off-o
rel={}
s=e.get_section_by_name(".rela.dyn")
for r in s.iter_relocations():
    if r["r_info_type"]==1027: rel[r["r_offset"]]=r["r_addend"]
inv={}
for k,v in rel.items(): inv.setdefault(v,[]).append(k)
for nm in names:
    pat=nm.encode()+b"\0"
    i=0; found=False
    while True:
        i=data.find(pat,i)
        if i<0: break
        if i>0 and data[i-1]==0:
            va=off2va(i)
            for loc in inv.get(va,[]):
                ti=loc-8
                for slot in inv.get(ti,[]):
                    vt=slot+8
                    fns=[]
                    k=vt
                    while k in rel and len(fns)<64:
                        fns.append(rel[k]); k+=8
                    print(nm,"typeinfo",hex(ti),"vtable",hex(vt),"n=",len(fns))
                    print("   ",[hex(x) for x in fns])
                    found=True
        i+=1
    if not found: print(nm,"not found")
